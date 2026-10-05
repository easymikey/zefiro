use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use kernel::{
    cmd::Cmds,
    message::Message,
    update::machine::{Driver, LoopCmd, LoopEffect, Machine},
};

use crate::{
    driver_thread::{Congestion, DriverThread, SendError, send, spawn_driver},
    driver_wait::{Inboxes, LoopInput},
    error::Error,
    jobs::{JobThread, Jobs, spawn_jobs, stash},
    registry::DriverRow,
    timers::Timers,
    watcher::FileStream,
};

const JOB_RESULTS: usize = 8;

#[derive(Debug)]
pub(crate) struct DriverLoop<D: Driver, J> {
    pub(crate) row: &'static DriverRow,
    pub(crate) inbox: Sender<Message>,
    pub(crate) heard: Receiver<D::Message>,
    pub(crate) seed: Option<D::Message>,
    pub(crate) jobs: Jobs<J, D::Message>,
}

struct Outlets<'a, D: Driver, J> {
    inbox: &'a Sender<Message>,
    full: &'a Congestion,
    row: &'static DriverRow,
    jobs: Jobs<J, D::Message>,
    results: Sender<D::Message>,
    worker: Option<JobThread<J>>,
    pending: Vec<J>,
    timers: Timers<D::Message>,
    files: FileStream<D::Message>,
}

impl<D, J> Outlets<'_, D, J>
where
    D: Driver,
    D::Message: Send + 'static,
    J: Ord + Send + 'static,
{
    fn hand_over(&mut self) -> Result<(), SendError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        if self.worker.is_none() {
            let started = spawn_jobs(self.row, self.results.clone(), self.jobs.run)
                .map_err(|_spawn| SendError::Closed)?;
            self.worker = Some(started);
        }
        let Some(worker) = &self.worker else {
            return Ok(());
        };
        while !self.pending.is_empty() {
            match worker.jobs.try_send(self.pending.remove(0)) {
                Ok(()) => {}
                Err(TrySendError::Full(job)) => {
                    self.pending.insert(0, job);
                    return Ok(());
                }
                Err(TrySendError::Disconnected(_job)) => {
                    return Err(SendError::Closed);
                }
            }
        }
        Ok(())
    }

    fn place(
        &mut self,
        effect: LoopEffect<<D as Driver>::Effect, J, D::Message>,
        driver: &mut D,
    ) -> Option<D::Message> {
        match effect {
            LoopEffect::Execute(effect) => driver.execute(effect),
            LoopEffect::Run(job) => {
                stash(&mut self.pending, job);
                None
            }
            LoopEffect::After { delay, message } => {
                if let Some(deadline) = Instant::now().checked_add(delay) {
                    self.timers.schedule(deadline, message);
                }
                None
            }
            LoopEffect::Watch { path, item } => self.files.watch(&path, item),
            LoopEffect::Unwatch(path) => self.files.unwatch(&path),
        }
    }
}

impl<D, J, E, X, M> DriverLoop<D, J>
where
    D: Driver<Effect = E> + Machine<Message = X, Effect = LoopCmd<E, J, X, M>>,
    X: Send + 'static,
    E: 'static,
    J: Ord + Send + 'static,
    M: Into<Message> + Send + 'static,
{
    pub(crate) fn spawn<C>(
        self,
        start: impl FnOnce() -> D + Send + 'static,
    ) -> Result<DriverThread<C>, Error>
    where
        C: Send + 'static,
        D::Message: From<Cmds<C>>,
    {
        let Self {
            row,
            inbox,
            heard,
            seed,
            jobs,
        } = self;
        let (results, finished) = bounded(JOB_RESULTS);
        spawn_driver(
            row,
            move |commands: &Receiver<C>,
                  inbox: &Sender<Message>,
                  full: &Congestion| {
                let mut outlets = Outlets {
                    inbox,
                    full,
                    row,
                    jobs,
                    results,
                    worker: None,
                    pending: Vec::new(),
                    timers: Timers::default(),
                    files: FileStream::Idle,
                };
                let inboxes = Inboxes {
                    commands,
                    heard,
                    finished,
                };
                let mut driver = start();
                let mut fed = |message| Self::feed(&mut driver, message, &mut outlets);
                if seed.is_none_or(|message| fed(message).is_ok()) {
                    Self::drive(driver, inboxes, &mut outlets);
                }
                if let Some(worker) = outlets.worker {
                    worker.stop();
                }
            },
            &inbox,
        )
    }

    fn drive<C>(
        mut driver: D,
        mut inboxes: Inboxes<'_, C, D::Message>,
        outlets: &mut Outlets<'_, D, J>,
    ) where
        D::Message: From<Cmds<C>>,
    {
        loop {
            let fed = match inboxes.wait(&outlets.files, outlets.timers.next_deadline())
            {
                LoopInput::Heard(message) => Self::feed(&mut driver, message, outlets),
                LoopInput::Due => Self::feed_due(&mut driver, outlets),
                LoopInput::Lost(source) => {
                    inboxes.lose(&source, &mut outlets.files);
                    Ok(())
                }
                LoopInput::Closed => return,
            };
            if fed.is_err() {
                return;
            }
        }
    }

    fn feed_due(
        driver: &mut D,
        outlets: &mut Outlets<'_, D, J>,
    ) -> Result<(), SendError> {
        for message in outlets.timers.take_due(Instant::now()) {
            Self::feed(driver, message, outlets)?;
        }
        Ok(())
    }

    fn feed(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, D, J>,
    ) -> Result<(), SendError> {
        Self::step(driver, message, outlets)?;
        outlets.hand_over()
    }

    fn step(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, D, J>,
    ) -> Result<(), SendError> {
        let Ok(cmd) = driver.transition(message) else {
            return Ok(());
        };
        let (effects, messages) = cmd.into_parts();
        for event in messages {
            send(outlets.inbox, outlets.full, event.into())?;
        }
        for effect in effects {
            if let Some(answer) = outlets.place(effect, driver) {
                Self::step(driver, answer, outlets)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    use audio::AudioDriver;
    use crossbeam_channel::{Receiver, Sender, bounded, never, unbounded};
    use kernel::{
        cmd::{AudioCmd, Cmd, Cmds},
        domain::{driver::DriverName, io_error::IoError, settings::AudioSettings},
        message::{AudioEvent, DriverEvent, Message},
        update::machine::{Driver, LoopCmd, LoopEffect, Machine, Unhandled},
    };

    use crate::{
        driver::{DriverLoop, Outlets},
        driver_thread::{Congestion, DriverThread, spawn_idle},
        jobs::Jobs,
        registry,
        timers::Timers,
        watcher::FileStream,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);
    const SHORT: Duration = Duration::from_millis(30);

    #[derive(Debug, PartialEq, Eq)]
    enum ProbeCmd {
        After { delay: Duration, tag: u8 },
        Watch(PathBuf),
        Announce,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ProbeMessage {
        Cmds(Cmds<ProbeCmd>),
        Fired(u8),
        Changed(Result<(), IoError>),
    }

    impl From<Cmds<ProbeCmd>> for ProbeMessage {
        fn from(cmds: Cmds<ProbeCmd>) -> Self {
            Self::Cmds(cmds)
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Note {
        Fired(u8),
        Changed(Result<(), IoError>),
    }

    enum ProbeEffect {
        Report(Note),
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum NoJob {}

    struct Probe {
        notes: Sender<Note>,
    }

    impl Machine for Probe {
        type Message = ProbeMessage;
        type Effect = LoopCmd<ProbeEffect, NoJob, ProbeMessage, Message>;

        fn transition(
            &mut self,
            message: ProbeMessage,
        ) -> Result<LoopCmd<ProbeEffect, NoJob, ProbeMessage, Message>, Unhandled>
        {
            Ok(match message {
                ProbeMessage::Cmds(cmds) => cmds
                    .cmds
                    .into_iter()
                    .map(|cmd| match cmd {
                        ProbeCmd::After { delay, tag } => {
                            Cmd::effect(LoopEffect::After {
                                delay,
                                message: ProbeMessage::Fired(tag),
                            })
                        }
                        ProbeCmd::Watch(path) => Cmd::effect(LoopEffect::Watch {
                            path,
                            item: ProbeMessage::Changed,
                        }),
                        ProbeCmd::Announce => Cmd::message(Message::Driver {
                            driver: DriverName::Config,
                            event: DriverEvent::Full,
                        }),
                    })
                    .fold(Cmd::none(), Cmd::then),
                ProbeMessage::Fired(tag) => Cmd::effect(LoopEffect::Execute(
                    ProbeEffect::Report(Note::Fired(tag)),
                )),
                ProbeMessage::Changed(result) => Cmd::effect(LoopEffect::Execute(
                    ProbeEffect::Report(Note::Changed(result)),
                )),
            })
        }
    }

    impl Driver for Probe {
        type Effect = ProbeEffect;

        fn execute(&mut self, effect: ProbeEffect) -> Option<ProbeMessage> {
            match effect {
                ProbeEffect::Report(note) => {
                    self.notes.send(note).unwrap();
                    None
                }
            }
        }
    }

    struct ProbeRun {
        thread: DriverThread<ProbeCmd>,
        notes: Receiver<Note>,
        reports: Receiver<Message>,
    }

    impl ProbeRun {
        fn start(heard: Receiver<ProbeMessage>) -> Self {
            let (inbox, reports) = unbounded();
            let (notes_sender, notes) = unbounded();
            let thread = DriverLoop::<Probe, NoJob> {
                row: registry::row(DriverName::Config),
                inbox,
                heard,
                seed: None,
                jobs: Jobs {
                    run: |job: NoJob| match job {},
                },
            }
            .spawn(move || Probe {
                notes: notes_sender,
            })
            .unwrap();
            Self {
                thread,
                notes,
                reports,
            }
        }

        fn send(&self, cmd: ProbeCmd) {
            self.thread.commands.send(cmd).unwrap();
        }

        fn stop(self) {
            drop(self.thread.commands);
            self.thread.handle.join().unwrap().unwrap();
            assert_eq!(
                self.reports.recv_timeout(RECV_TIMEOUT),
                Ok(Message::Driver {
                    driver: DriverName::Config,
                    event: DriverEvent::Stopped
                })
            );
        }
    }

    #[test]
    fn an_after_arrives_once_its_delay_has_passed() {
        let run = ProbeRun::start(never());
        let sent_at = Instant::now();

        run.send(ProbeCmd::After {
            delay: SHORT,
            tag: 1,
        });

        assert_eq!(run.notes.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(1)));
        assert!(sent_at.elapsed() >= SHORT);
        run.stop();
    }

    #[test]
    fn a_second_after_of_the_same_kind_replaces_the_first() {
        let run = ProbeRun::start(never());

        run.send(ProbeCmd::After {
            delay: SHORT,
            tag: 2,
        });
        run.send(ProbeCmd::After {
            delay: SHORT * 2,
            tag: 1,
        });

        assert_eq!(run.notes.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(1)));
        assert!(run.notes.recv_timeout(SHORT * 4).is_err());
        run.stop();
    }

    #[test]
    fn a_write_under_a_watched_directory_arrives_as_a_change() {
        let directory = tempfile::tempdir().unwrap();
        let run = ProbeRun::start(never());
        run.send(ProbeCmd::Watch(directory.path().to_path_buf()));
        run.send(ProbeCmd::After {
            delay: Duration::ZERO,
            tag: 0,
        });
        assert_eq!(run.notes.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(0)));

        std::fs::write(directory.path().join("probe.txt"), "changed").unwrap();

        assert_eq!(
            run.notes.recv_timeout(Duration::from_secs(5)),
            Ok(Note::Changed(Ok(())))
        );
        run.stop();
    }

    #[test]
    fn a_dropped_heard_sender_does_not_end_the_loop() {
        let (heard_sender, heard) = bounded(1);
        drop(heard_sender);
        let run = ProbeRun::start(heard);

        run.send(ProbeCmd::After {
            delay: Duration::ZERO,
            tag: 3,
        });

        assert_eq!(run.notes.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(3)));
        run.stop();
    }

    #[test]
    fn a_final_report_into_a_full_inbox_raises_congestion() {
        let (inbox, reports) = bounded(1);
        let filler = Message::Driver {
            driver: DriverName::Library,
            event: DriverEvent::Stopped,
        };
        inbox.send(filler.clone()).unwrap();
        let thread =
            spawn_idle::<ProbeCmd>(registry::row(DriverName::Config), &inbox).unwrap();

        drop(thread.commands);
        let deadline = Instant::now() + RECV_TIMEOUT;
        let raised = std::iter::repeat_with(|| thread.full.take())
            .take_while(|_| Instant::now() < deadline)
            .any(|raised| raised);

        assert!(raised);
        assert_eq!(reports.recv_timeout(RECV_TIMEOUT), Ok(filler));
        assert_eq!(
            reports.recv_timeout(RECV_TIMEOUT),
            Ok(Message::Driver {
                driver: DriverName::Config,
                event: DriverEvent::Stopped
            })
        );
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_driver_without_jobs_starts_no_worker() {
        let (inbox, _reports) = unbounded();
        let full = Congestion::default();
        let (results, _finished) = unbounded();
        let mut outlets = Outlets::<Probe, NoJob> {
            inbox: &inbox,
            full: &full,
            row: registry::row(DriverName::Config),
            jobs: Jobs {
                run: |job: NoJob| match job {},
            },
            results,
            worker: None,
            pending: Vec::new(),
            timers: Timers::default(),
            files: FileStream::Idle,
        };

        assert_eq!(outlets.hand_over(), Ok(()));
        assert!(outlets.worker.is_none());
    }

    fn start_audio_driver() -> (DriverThread<AudioCmd>, Receiver<Message>) {
        let (inbox, sent) = unbounded();
        let (deck_sender, heard) = bounded(64);
        let settings = AudioSettings::default();
        let row = registry::row(DriverName::Audio);
        let jobs = Jobs {
            run: audio::deck::job::AudioJob::run,
        };
        let thread = DriverLoop::<AudioDriver, _> {
            row,
            inbox,
            heard,
            seed: None,
            jobs,
        }
        .spawn(move || AudioDriver::new(settings, deck_sender).0)
        .unwrap();
        (thread, sent)
    }

    fn is_devices_answer(message: &Result<Message, impl Sized>) -> bool {
        matches!(
            message,
            Ok(Message::Audio(
                AudioEvent::DevicesListed(_) | AudioEvent::Error(_)
            ))
        )
    }

    #[test]
    #[ignore = "hardware: opens the output device"]
    fn a_listed_devices_answer_comes_back_through_the_deck_inbox() {
        let (thread, sent) = start_audio_driver();

        thread.commands.send(AudioCmd::ListDevices).unwrap();
        assert!(is_devices_answer(
            &sent.recv_timeout(Duration::from_secs(5))
        ));
        assert!(sent.try_recv().is_err());

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    #[ignore = "hardware: opens the output device"]
    fn a_muted_start_reports_nothing_until_a_command_arrives() {
        let (thread, sent) = start_audio_driver();

        thread::sleep(Duration::from_millis(250));
        assert!(sent.try_recv().is_err());

        thread.commands.send(AudioCmd::ListDevices).unwrap();
        assert!(is_devices_answer(
            &sent.recv_timeout(Duration::from_secs(5))
        ));

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_closed_mailbox_ends_the_driver_loop() {
        let ProbeRun {
            thread, reports, ..
        } = ProbeRun::start(never());
        drop(reports);

        thread.commands.send(ProbeCmd::Announce).unwrap();

        let (finished, joined) = bounded(1);
        let DriverThread {
            commands, handle, ..
        } = thread;
        thread::spawn(move || {
            finished.send(handle.join().is_ok()).unwrap();
        });
        assert_eq!(joined.recv_timeout(Duration::from_secs(5)), Ok(true));
        drop(commands);
    }
}
