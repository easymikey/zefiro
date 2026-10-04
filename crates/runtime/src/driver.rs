use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, SendError, Sender, TrySendError, bounded};
use kernel::{
    cmd::{Cmd, Cmds},
    domain::driver::DriverError,
    message::{DriverEvent, Message},
    update::machine::{Driver, Machine},
};

use crate::{
    driver_wait::{Inboxes, LoopInput},
    error::Error,
    jobs::{Jobs, spawn_jobs, stash},
    outbox::{Congestion, Outbox},
    registry::DriverRow,
    watcher::{Changed, FileStream},
};

#[derive(Debug)]
pub(crate) struct DriverThread<C> {
    pub(crate) commands: Sender<C>,
    pub(crate) handle: JoinHandle<Result<(), SendError<Message>>>,
    pub(crate) full: Congestion,
}

const INBOX: usize = 64;
const JOB_RESULTS: usize = 8;

#[cfg(test)]
pub(crate) fn spawn_idle<C: Send + 'static>(
    row: &DriverRow,
    inbox: &Sender<Message>,
) -> Result<DriverThread<C>, Error> {
    spawn_driver(
        row,
        |inbox: &Receiver<C>, _: &Outbox<Message>| while inbox.recv().is_ok() {},
        inbox,
    )
}

pub(crate) fn spawn_driver<C, F, R>(
    row: &DriverRow,
    run: R,
    inbox: &Sender<Message>,
) -> Result<DriverThread<C>, Error>
where
    C: Send + 'static,
    F: Send + 'static,
    R: FnOnce(&Receiver<C>, &Outbox<F>) + Send + 'static,
{
    let (commands, command_inbox): (Sender<C>, Receiver<C>) = bounded(INBOX);
    let full = Congestion::default();
    let outbox = Outbox::new(inbox.clone(), full.clone());
    let inbox = inbox.clone();
    let driver = row.driver;
    let handle = thread::Builder::new()
        .name(row.thread_name.to_owned())
        .spawn(move || -> Result<(), SendError<Message>> {
            let result =
                catch_unwind(AssertUnwindSafe(|| run(&command_inbox, &outbox)));
            let report = match result {
                Ok(()) => DriverEvent::Stopped,
                Err(_payload) => DriverEvent::Died(DriverError::Panicked),
            };
            inbox.send(Message::Driver {
                driver,
                event: report,
            })
        })
        .map_err(|source| Error::Spawn { driver, source })?;
    Ok(DriverThread {
        commands,
        handle,
        full,
    })
}

#[derive(Debug)]
pub(crate) enum LoopEffect<E, J, M> {
    Execute(E),
    Run(J),
    After { delay: Duration, message: M },
    Watch { path: PathBuf, item: Changed<M> },
    Unwatch(PathBuf),
}

#[derive(Debug)]
pub(crate) struct DriverLoop<D: Driver, J> {
    pub(crate) row: &'static DriverRow,
    pub(crate) inbox: Sender<Message>,
    pub(crate) heard: Receiver<D::Message>,
    pub(crate) seed: Option<D::Message>,
    pub(crate) jobs: Jobs<<D as Driver>::Effect, J, D::Message>,
}

struct LoopTimer<M> {
    deadline: Instant,
    message: M,
}

type Split<D, J> = fn(
    <D as Driver>::Effect,
) -> LoopEffect<<D as Driver>::Effect, J, <D as Machine>::Message>;

struct Outlets<'a, D: Driver, J, O> {
    outbox: &'a Outbox<O>,
    jobs: &'a Sender<J>,
    split: Split<D, J>,
    pending: Vec<J>,
    timers: Vec<LoopTimer<D::Message>>,
    files: FileStream<D::Message>,
}

impl<D: Driver, J, O> Outlets<'_, D, J, O> {
    fn hand_over(&mut self) -> Result<(), crate::outbox::SendError> {
        while !self.pending.is_empty() {
            match self.jobs.try_send(self.pending.remove(0)) {
                Ok(()) => {}
                Err(TrySendError::Full(job)) => {
                    self.pending.insert(0, job);
                    return Ok(());
                }
                Err(TrySendError::Disconnected(_job)) => {
                    return Err(crate::outbox::SendError::Closed);
                }
            }
        }
        Ok(())
    }

    fn place(
        &mut self,
        effect: <D as Driver>::Effect,
        driver: &mut D,
    ) -> Option<D::Message> {
        match (self.split)(effect) {
            LoopEffect::Execute(effect) => driver.execute(effect),
            LoopEffect::Run(job) => {
                stash(&mut self.pending, job);
                None
            }
            LoopEffect::After { delay, message } => {
                if let Some(deadline) = Instant::now().checked_add(delay) {
                    self.timers.push(LoopTimer { deadline, message });
                }
                None
            }
            LoopEffect::Watch { path, item } => self.files.watch(&path, item),
            LoopEffect::Unwatch(path) => self.files.unwatch(&path),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.timers.iter().map(|timer| timer.deadline).min()
    }

    fn take_due(&mut self, now: Instant) -> Vec<D::Message> {
        self.timers.sort_by_key(|timer| timer.deadline);
        let due = self.timers.partition_point(|timer| timer.deadline <= now);
        self.timers
            .drain(..due)
            .map(|timer| timer.message)
            .collect()
    }
}

impl<D, J, M> DriverLoop<D, J>
where
    D: Driver + Machine<Effect = Cmd<<D as Driver>::Effect, M>>,
    D::Message: Send + 'static,
    <D as Driver>::Effect: 'static,
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
        let worker = spawn_jobs(row, results, jobs.run)?;
        let split = jobs.split;
        spawn_driver(
            row,
            move |commands: &Receiver<C>, outbox: &Outbox<M>| {
                let mut outlets = Outlets {
                    outbox,
                    jobs: &worker.jobs,
                    split,
                    pending: Vec::new(),
                    timers: Vec::new(),
                    files: FileStream::idle(),
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
                worker.stop();
            },
            &inbox,
        )
    }

    fn drive<C>(
        mut driver: D,
        mut inboxes: Inboxes<'_, C, D::Message>,
        outlets: &mut Outlets<'_, D, J, M>,
    ) where
        D::Message: From<Cmds<C>>,
    {
        loop {
            let fed = match inboxes.wait(&outlets.files, outlets.next_deadline()) {
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
        outlets: &mut Outlets<'_, D, J, M>,
    ) -> Result<(), crate::outbox::SendError> {
        for message in outlets.take_due(Instant::now()) {
            Self::feed(driver, message, outlets)?;
        }
        Ok(())
    }

    fn feed(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, D, J, M>,
    ) -> Result<(), crate::outbox::SendError> {
        Self::step(driver, message, outlets)?;
        outlets.hand_over()
    }

    fn step(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, D, J, M>,
    ) -> Result<(), crate::outbox::SendError> {
        let Ok(cmd) = driver.transition(message) else {
            return Ok(());
        };
        let (effects, messages) = cmd.into_parts();
        for event in messages {
            outlets.outbox.send(event)?;
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
        domain::{
            driver::{DriverError, DriverName},
            io_error::IoError,
            settings::AudioSettings,
        },
        message::{AudioEvent, DriverEvent, Message},
        update::machine::{Driver, Machine, Unhandled},
    };

    use crate::{
        driver::{DriverLoop, DriverThread, LoopEffect, spawn_driver, spawn_idle},
        jobs::Jobs,
        outbox::Outbox,
        registry,
        spawn::audio_thread::audio_split,
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
        After { delay: Duration, tag: u8 },
        Watch(PathBuf),
        Report(Note),
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum NoJob {}

    struct Probe {
        notes: Sender<Note>,
    }

    impl Machine for Probe {
        type Message = ProbeMessage;
        type Effect = Cmd<ProbeEffect, Message>;

        fn transition(
            &mut self,
            message: ProbeMessage,
        ) -> Result<Cmd<ProbeEffect, Message>, Unhandled> {
            Ok(match message {
                ProbeMessage::Cmds(cmds) => cmds
                    .cmds
                    .into_iter()
                    .map(|cmd| match cmd {
                        ProbeCmd::After { delay, tag } => {
                            Cmd::effect(ProbeEffect::After { delay, tag })
                        }
                        ProbeCmd::Watch(path) => Cmd::effect(ProbeEffect::Watch(path)),
                        ProbeCmd::Announce => Cmd::message(Message::Driver {
                            driver: DriverName::Config,
                            event: DriverEvent::Full,
                        }),
                    })
                    .fold(Cmd::none(), Cmd::then),
                ProbeMessage::Fired(tag) => {
                    Cmd::effect(ProbeEffect::Report(Note::Fired(tag)))
                }
                ProbeMessage::Changed(result) => {
                    Cmd::effect(ProbeEffect::Report(Note::Changed(result)))
                }
            })
        }
    }

    impl Driver for Probe {
        type Effect = ProbeEffect;

        fn execute(&mut self, effect: ProbeEffect) -> Option<ProbeMessage> {
            match effect {
                ProbeEffect::Report(note) => match self.notes.send(note) {
                    Ok(()) | Err(_) => None,
                },
                ProbeEffect::After { .. } | ProbeEffect::Watch(_) => None,
            }
        }
    }

    fn probe_split(
        effect: ProbeEffect,
    ) -> LoopEffect<ProbeEffect, NoJob, ProbeMessage> {
        match effect {
            ProbeEffect::After { delay, tag } => LoopEffect::After {
                delay,
                message: ProbeMessage::Fired(tag),
            },
            ProbeEffect::Watch(path) => LoopEffect::Watch {
                path,
                item: ProbeMessage::Changed,
            },
            report @ ProbeEffect::Report(_) => LoopEffect::Execute(report),
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
                    split: probe_split,
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
    fn two_afters_arrive_in_deadline_order() {
        let run = ProbeRun::start(never());

        run.send(ProbeCmd::After {
            delay: SHORT * 4,
            tag: 2,
        });
        run.send(ProbeCmd::After {
            delay: SHORT,
            tag: 1,
        });

        let notes: Vec<_> = (0..2)
            .map(|_round| run.notes.recv_timeout(RECV_TIMEOUT))
            .collect();
        assert_eq!(notes, vec![Ok(Note::Fired(1)), Ok(Note::Fired(2))]);
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

    fn start_audio_driver() -> (DriverThread<AudioCmd>, Receiver<Message>) {
        let (inbox, sent) = unbounded();
        let (deck_sender, heard) = bounded(64);
        let settings = AudioSettings::default();
        let row = registry::row(DriverName::Audio);
        let jobs = Jobs {
            split: audio_split,
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
    fn a_closed_outbox_ends_the_driver_loop() {
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

    #[test]
    fn a_panicking_driver_becomes_died() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(DriverName::Audio),
            |_inbox: &Receiver<()>, _outbox: &Outbox<Message>| panic!("boom"),
            &inbox,
        )
        .unwrap();

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: DriverName::Audio,
                event: DriverEvent::Died(DriverError::Panicked)
            }
        );
    }

    #[test]
    fn a_returning_driver_becomes_stopped() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(DriverName::Library),
            |_inbox: &Receiver<()>, _outbox: &Outbox<Message>| {},
            &inbox,
        )
        .unwrap();

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: DriverName::Library,
                event: DriverEvent::Stopped
            }
        );
    }

    #[test]
    fn a_closed_inbox_stops_the_driver() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(DriverName::Macos),
            |inbox: &Receiver<()>, _outbox: &Outbox<Message>| {
                assert!(inbox.recv().is_err());
            },
            &inbox,
        )
        .unwrap();
        drop(thread.commands);

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: DriverName::Macos,
                event: DriverEvent::Stopped
            }
        );
    }

    #[test]
    fn an_idle_driver_stops_once_its_inbox_closes() {
        let (inbox, reports) = unbounded();
        let thread =
            spawn_idle::<()>(registry::row(DriverName::Audio), &inbox).unwrap();
        drop(thread.commands);

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: DriverName::Audio,
                event: DriverEvent::Stopped
            }
        );
    }
}
