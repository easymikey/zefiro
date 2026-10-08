use std::{collections::HashMap, panic::resume_unwind, time::Instant};

use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    cmd::Cmds,
    domain::driver::DriverError,
    message::Message,
    update::machine::{Driver, LoopCmd, Machine},
};

use crate::{
    driver_thread::{Congestion, DriverThread, Halt, send, spawn_driver},
    driver_wait::{LoopInput, WaitSources},
    error::SpawnError,
    outlets::Outlets,
    registry::DriverRow,
    timers::Timers,
    watcher::FileStream,
};

const JOB_RESULTS: usize = 8;

#[derive(Debug)]
pub(crate) struct DriverLoop<D: Driver, J> {
    pub(crate) row: &'static DriverRow,
    pub(crate) inbox: Sender<Message>,
    pub(crate) callback_receiver: Receiver<D::Message>,
    pub(crate) message: Option<D::Message>,
    pub(crate) run_job: fn(J) -> D::Message,
}

impl<D, J, E, X, M> DriverLoop<D, J>
where
    D: Driver<Effect = E> + Machine<Message = X, Effect = LoopCmd<E, J, X, M>>,
    X: Send + 'static,
    E: 'static,
    J: Send + 'static,
    M: Into<Message> + Send + 'static,
{
    pub(crate) fn spawn<C>(
        self,
        start: impl FnOnce() -> D + Send + 'static,
    ) -> Result<DriverThread<C>, SpawnError>
    where
        C: Send + 'static,
        D::Message: From<Cmds<C>>,
    {
        let Self {
            row,
            inbox,
            callback_receiver,
            message: seed,
            run_job,
        } = self;
        let (result_sender, finished) = bounded(JOB_RESULTS);
        spawn_driver(
            row,
            move |cmd_receiver: &Receiver<C>,
                  inbox: &Sender<Message>,
                  congestion: &Congestion| {
                let mut outlets = Outlets {
                    inbox,
                    congestion,
                    row,
                    run_job,
                    result_sender,
                    workers: HashMap::new(),
                    pending: Vec::new(),
                    timers: Timers::default(),
                    file_stream: FileStream::Idle,
                };
                let wait_sources = WaitSources {
                    cmd_receiver,
                    callback_receiver,
                    finished_receiver: finished,
                };
                let mut driver = start();
                let seeded = seed.map_or(Ok(()), |message| {
                    Self::feed(&mut driver, message, &mut outlets)
                });
                match seeded
                    .and_then(|()| Self::drive(driver, wait_sources, &mut outlets))
                {
                    Err(Halt::Spawn(error)) => Err(DriverError::from(&error)),
                    Ok(()) | Err(Halt::Inbox(_)) => Ok(()),
                }
            },
            &inbox,
        )
    }

    fn drive<C>(
        mut driver: D,
        mut wait_sources: WaitSources<'_, C, D::Message>,
        outlets: &mut Outlets<'_, D, J>,
    ) -> Result<(), Halt>
    where
        D::Message: From<Cmds<C>>,
    {
        loop {
            let fed = match wait_sources
                .wait(&outlets.file_stream, outlets.timers.next_deadline())
            {
                LoopInput::Message(message) => {
                    Self::feed(&mut driver, message, outlets)
                }
                LoopInput::Due => Ok(()),
                LoopInput::Panicked(payload) => resume_unwind(payload),
                LoopInput::Lost(source) => wait_sources
                    .lose(&source, &mut outlets.file_stream)
                    .map_or(Ok(()), |lost| Self::feed(&mut driver, lost, outlets)),
                LoopInput::Closed => return Ok(()),
            }
            .and_then(|()| Self::feed_due(&mut driver, outlets));
            fed?;
        }
    }

    fn feed_due(driver: &mut D, outlets: &mut Outlets<'_, D, J>) -> Result<(), Halt> {
        let due = outlets.timers.take_due(Instant::now());
        due.into_iter()
            .try_for_each(|message| Self::feed(driver, message, outlets))
    }

    fn feed(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, D, J>,
    ) -> Result<(), Halt> {
        Self::step(driver, message, outlets)?;
        outlets.hand_over()?;
        Ok(())
    }

    fn step(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, D, J>,
    ) -> Result<(), Halt> {
        let Ok(cmd) = driver.transition(message) else {
            return Ok(());
        };
        let (effects, messages) = cmd.into_parts();
        for event in messages {
            send(outlets.inbox, outlets.congestion, event.into())?;
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
pub(crate) mod tests {
    use std::{
        collections::HashMap,
        convert::Infallible,
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    use crossbeam_channel::{Receiver, Sender, bounded, never, unbounded};
    use kernel::{
        cmd::{Cmd, Cmds},
        domain::{
            driver::{DriverError, DriverName},
            io_error::IoError,
        },
        message::{DriverEvent, Message},
        update::machine::{Driver, LoopCmd, LoopEffect, Machine, Unhandled},
    };

    use crate::{
        driver::DriverLoop,
        driver_thread::{Congestion, DriverThread},
        outlets::Outlets,
        registry,
        runtime::Runtime,
        spawn::tests::spawn_idle,
        timers::Timers,
        watcher::FileStream,
    };

    pub(crate) const RECV_TIMEOUT: Duration = Duration::from_secs(1);
    const SHORT: Duration = Duration::from_millis(30);
    pub(crate) const LONG_JOB: Duration = Runtime::DRAIN.saturating_mul(2);

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
        Tick,
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
        doorbell_sender: Sender<Note>,
    }

    impl Machine for Probe {
        type Message = ProbeMessage;
        type Effect = LoopCmd<ProbeEffect, NoJob, ProbeMessage, Message>;

        fn transition(
            &mut self,
            probe_message: ProbeMessage,
        ) -> Result<LoopCmd<ProbeEffect, NoJob, ProbeMessage, Message>, Unhandled>
        {
            Ok(match probe_message {
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
                            changed: ProbeMessage::Changed,
                        }),
                        ProbeCmd::Announce => Cmd::message(Message::Driver {
                            driver_name: DriverName::Config,
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
                ProbeMessage::Tick => {
                    thread::sleep(Duration::from_millis(1));
                    Cmd::none()
                }
            })
        }
    }

    impl Driver for Probe {
        type Effect = ProbeEffect;

        fn execute(&mut self, probe_effect: ProbeEffect) -> Option<ProbeMessage> {
            match probe_effect {
                ProbeEffect::Report(note) => {
                    self.doorbell_sender.send(note).unwrap();
                    None
                }
            }
        }
    }

    struct ProbeRun {
        thread: DriverThread<ProbeCmd>,
        doorbell: Receiver<Note>,
        report_receiver: Receiver<Message>,
    }

    impl ProbeRun {
        fn start(callback_receiver: Receiver<ProbeMessage>) -> Self {
            let (inbox, report_receiver) = unbounded();
            let (doorbell_sender, doorbell) = unbounded();
            let thread = DriverLoop::<Probe, NoJob> {
                row: registry::row(DriverName::Config),
                inbox,
                callback_receiver,
                message: None,
                run_job: |job: NoJob| match job {},
            }
            .spawn(move || Probe { doorbell_sender })
            .unwrap();
            Self {
                thread,
                doorbell,
                report_receiver,
            }
        }

        fn send(&self, probe_cmd: ProbeCmd) {
            self.thread.cmd_sender.send(probe_cmd).unwrap();
        }

        fn stop(self) {
            drop(self.thread.cmd_sender);
            self.thread.handle.join().unwrap();
            assert_eq!(
                self.report_receiver.recv_timeout(RECV_TIMEOUT),
                Ok(Message::Driver {
                    driver_name: DriverName::Config,
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

        assert_eq!(run.doorbell.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(1)));
        assert!(sent_at.elapsed() >= SHORT);
        run.stop();
    }

    #[test]
    fn a_due_timer_fires_while_inputs_keep_arriving() {
        let (ticks, callback_receiver) = unbounded();
        for _ in 0..100 {
            ticks.send(ProbeMessage::Tick).unwrap();
        }
        ticks.send(ProbeMessage::Fired(9)).unwrap();
        let run = ProbeRun::start(callback_receiver);

        run.send(ProbeCmd::After {
            delay: SHORT,
            tag: 1,
        });

        assert_eq!(run.doorbell.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(1)));
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

        assert_eq!(run.doorbell.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(1)));
        assert!(run.doorbell.recv_timeout(SHORT * 4).is_err());
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
        assert_eq!(run.doorbell.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(0)));

        std::fs::write(directory.path().join("probe.txt"), "changed").unwrap();

        assert_eq!(
            run.doorbell.recv_timeout(Duration::from_secs(5)),
            Ok(Note::Changed(Ok(())))
        );
        run.stop();
    }

    #[test]
    fn a_dropped_callback_sender_does_not_end_the_loop() {
        let (callback_sender, callback_receiver) = bounded(1);
        drop(callback_sender);
        let run = ProbeRun::start(callback_receiver);

        run.send(ProbeCmd::After {
            delay: Duration::ZERO,
            tag: 3,
        });

        assert_eq!(run.doorbell.recv_timeout(RECV_TIMEOUT), Ok(Note::Fired(3)));
        run.stop();
    }

    #[test]
    fn a_final_report_into_a_full_inbox_raises_congestion() {
        let (inbox, reports) = bounded(1);
        let filler_message = Message::Driver {
            driver_name: DriverName::Library,
            event: DriverEvent::Stopped,
        };
        inbox.send(filler_message.clone()).unwrap();
        let thread =
            spawn_idle::<ProbeCmd>(registry::row(DriverName::Config), &inbox).unwrap();

        drop(thread.cmd_sender);
        let deadline = Instant::now() + RECV_TIMEOUT;
        let raised = std::iter::repeat_with(|| thread.congestion.take())
            .take_while(|_| Instant::now() < deadline)
            .any(|raised| raised);

        assert!(raised);
        assert_eq!(reports.recv_timeout(RECV_TIMEOUT), Ok(filler_message));
        assert_eq!(
            reports.recv_timeout(RECV_TIMEOUT),
            Ok(Message::Driver {
                driver_name: DriverName::Config,
                event: DriverEvent::Stopped
            })
        );
        thread.handle.join().unwrap();
    }

    #[test]
    fn a_driver_without_jobs_starts_no_worker() {
        let (inbox, _reports) = unbounded();
        let congestion = Congestion::default();
        let (result_sender, _result_receiver) = unbounded();
        let mut outlets = Outlets::<Probe, NoJob> {
            inbox: &inbox,
            congestion: &congestion,
            row: registry::row(DriverName::Config),
            run_job: |job: NoJob| match job {},
            result_sender,
            workers: HashMap::new(),
            pending: Vec::new(),
            timers: Timers::default(),
            file_stream: FileStream::Idle,
        };

        outlets.hand_over().unwrap();
        assert!(outlets.workers.is_empty());
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub(crate) enum Nap {
        Long,
        Brief,
    }

    pub(crate) enum NapperMessage {
        Cmds(Cmds<Nap>),
        Woke(Nap),
    }

    impl From<Cmds<Nap>> for NapperMessage {
        fn from(cmds: Cmds<Nap>) -> Self {
            Self::Cmds(cmds)
        }
    }

    pub(crate) struct Napper;

    impl Machine for Napper {
        type Message = NapperMessage;
        type Effect = LoopCmd<Infallible, Nap, NapperMessage, Message>;

        fn transition(
            &mut self,
            napper_message: NapperMessage,
        ) -> Result<LoopCmd<Infallible, Nap, NapperMessage, Message>, Unhandled>
        {
            Ok(match napper_message {
                NapperMessage::Cmds(cmds) => cmds
                    .cmds
                    .into_iter()
                    .map(|nap| Cmd::effect(LoopEffect::Run(nap)))
                    .fold(Cmd::none(), Cmd::then),
                NapperMessage::Woke(_) => Cmd::none(),
            })
        }
    }

    impl Driver for Napper {
        type Effect = Infallible;

        fn execute(&mut self, effect: Infallible) -> Option<NapperMessage> {
            match effect {}
        }
    }

    #[test]
    fn a_driver_stops_while_its_job_is_still_running() {
        let (inbox, reports) = unbounded();
        let thread = DriverLoop::<Napper, Nap> {
            row: registry::row(DriverName::Config),
            inbox,
            callback_receiver: never(),
            message: None,
            run_job: |nap: Nap| match nap {
                Nap::Long => {
                    thread::sleep(LONG_JOB);
                    NapperMessage::Woke(Nap::Long)
                }
                Nap::Brief => NapperMessage::Woke(Nap::Brief),
            },
        }
        .spawn(|| Napper)
        .unwrap();
        thread.cmd_sender.send(Nap::Long).unwrap();
        thread::sleep(SHORT);

        let asked = Instant::now();
        drop(thread.cmd_sender);
        thread.handle.join().unwrap();

        assert!(asked.elapsed() < Runtime::DRAIN);
        assert_eq!(
            reports.recv_timeout(RECV_TIMEOUT),
            Ok(Message::Driver {
                driver_name: DriverName::Config,
                event: DriverEvent::Stopped
            })
        );
    }

    #[test]
    fn a_panicking_job_makes_its_driver_report_died() {
        let (inbox, reports) = unbounded();
        let thread = DriverLoop::<Napper, Nap> {
            row: registry::row(DriverName::Config),
            inbox,
            callback_receiver: never(),
            message: None,
            run_job: |_nap: Nap| -> NapperMessage {
                thread::sleep(SHORT);
                panic!("the job panics after it started")
            },
        }
        .spawn(|| Napper)
        .unwrap();

        thread.cmd_sender.send(Nap::Long).unwrap();

        assert_eq!(
            reports.recv_timeout(RECV_TIMEOUT),
            Ok(Message::Driver {
                driver_name: DriverName::Config,
                event: DriverEvent::Died(DriverError::Panicked)
            })
        );
        drop(thread.cmd_sender);
        thread.handle.join().unwrap();
    }

    #[test]
    fn a_closed_inbox_ends_the_driver_loop() {
        let ProbeRun {
            thread,
            report_receiver,
            ..
        } = ProbeRun::start(never());
        drop(report_receiver);

        thread.cmd_sender.send(ProbeCmd::Announce).unwrap();

        let (finished, joined) = bounded(1);
        let DriverThread {
            cmd_sender, handle, ..
        } = thread;
        thread::spawn(move || {
            finished.send(handle.join().is_ok()).unwrap();
        });
        assert_eq!(joined.recv_timeout(Duration::from_secs(5)), Ok(true));
        drop(cmd_sender);
    }
}
