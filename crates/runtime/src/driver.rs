use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    thread::{self, JoinHandle},
    time::Instant,
};

use crossbeam_channel::{Receiver, SendError, Sender, TrySendError, bounded, select};
use kernel::{
    Cmd,
    Cmds,
    Congestion,
    DriverEvent,
    Message,
    Outbox,
    domain::DriverError,
    update::{Driver, Machine},
};

use crate::{
    error::Error,
    jobs::{Jobs, spawn_jobs, stash},
    registry::DriverRow,
};

pub(crate) type Exit = Result<(), SendError<Message>>;

#[derive(Debug)]
pub(crate) struct DriverThread<C> {
    pub(crate) commands: Sender<C>,
    pub(crate) handle: JoinHandle<Exit>,
    pub(crate) full_edge: Congestion,
}

const INBOX: usize = 64;
const JOB_RESULTS: usize = 8;

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
    let full_edge = Congestion::default();
    let outbox = Outbox::new(inbox.clone(), full_edge.clone());
    let report_sender = inbox.clone();
    let driver = row.driver;
    let handle = thread::Builder::new()
        .name(row.thread_name.to_owned())
        .spawn(move || -> Exit {
            let result =
                catch_unwind(AssertUnwindSafe(|| run(&command_inbox, &outbox)));
            let report = match result {
                Ok(()) => DriverEvent::Stopped,
                Err(_payload) => DriverEvent::Died(DriverError::Panicked),
            };
            report_sender.send(Message::Driver {
                driver,
                event: report,
            })
        })
        .map_err(|source| Error::Spawn { driver, source })?;
    Ok(DriverThread {
        commands,
        handle,
        full_edge,
    })
}

#[derive(Debug)]
pub(crate) struct DriverLoop<D: Driver, J> {
    pub(crate) row: &'static DriverRow,
    pub(crate) inbox: Sender<Message>,
    pub(crate) heard: Receiver<D::Message>,
    pub(crate) jobs: Jobs<<D as Driver>::Effect, J, D::Message>,
}

struct Outlets<'a, E, J, M> {
    outbox: &'a Outbox<M>,
    jobs: &'a Sender<J>,
    pick: fn(E) -> Result<J, E>,
    pending: Vec<J>,
}

impl<E, J, M> Outlets<'_, E, J, M> {
    fn hand_over(&mut self) -> Result<(), kernel::SendError> {
        while !self.pending.is_empty() {
            match self.jobs.try_send(self.pending.remove(0)) {
                Ok(()) => {}
                Err(TrySendError::Full(job)) => {
                    self.pending.insert(0, job);
                    return Ok(());
                }
                Err(TrySendError::Disconnected(_job)) => {
                    return Err(kernel::SendError::Closed);
                }
            }
        }
        Ok(())
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
            jobs,
        } = self;
        let (results, finished) = bounded(JOB_RESULTS);
        let worker = spawn_jobs(row, results, jobs.run)?;
        let pick = jobs.pick;
        spawn_driver(
            row,
            move |commands: &Receiver<C>, outbox: &Outbox<M>| {
                let mut outlets = Outlets {
                    outbox,
                    jobs: &worker.jobs,
                    pick,
                    pending: Vec::new(),
                };
                Self::drive(start(), (commands, &heard, &finished), &mut outlets);
                worker.stop();
            },
            &inbox,
        )
    }

    fn drive<C>(
        mut driver: D,
        inboxes: (&Receiver<C>, &Receiver<D::Message>, &Receiver<D::Message>),
        outlets: &mut Outlets<'_, <D as Driver>::Effect, J, M>,
    ) where
        D::Message: From<Cmds<C>>,
    {
        let (commands, heard, finished) = inboxes;
        loop {
            let message = select! {
                recv(commands) -> received => match received {
                    Ok(first) => D::Message::from(gather(first, commands)),
                    Err(_) => return,
                },
                recv(heard) -> received => match received {
                    Ok(message) => message,
                    Err(_) => return,
                },
                recv(finished) -> received => match received {
                    Ok(message) => message,
                    Err(_) => return,
                },
            };
            let stepped = Self::step(&mut driver, message, outlets)
                .and_then(|()| outlets.hand_over());
            if stepped.is_err() {
                return;
            }
        }
    }

    fn step(
        driver: &mut D,
        message: D::Message,
        outlets: &mut Outlets<'_, <D as Driver>::Effect, J, M>,
    ) -> Result<(), kernel::SendError> {
        let Ok(cmd) = driver.transition(message) else {
            return Ok(());
        };
        let (effects, messages) = cmd.into_parts();
        for event in messages {
            ignore_full(outlets.outbox.send(event))?;
        }
        for effect in effects {
            match (outlets.pick)(effect) {
                Ok(job) => stash(&mut outlets.pending, job),
                Err(effect) => {
                    if let Some(answer) = driver.execute(effect) {
                        Self::step(driver, answer, outlets)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn gather<C>(first: C, commands: &Receiver<C>) -> Cmds<C> {
    let mut cmds = vec![first];
    cmds.extend(commands.try_iter());
    Cmds {
        cmds,
        at: Instant::now(),
    }
}

fn ignore_full(sent: Result<(), kernel::SendError>) -> Result<(), kernel::SendError> {
    match sent {
        Err(kernel::SendError::Closed) => Err(kernel::SendError::Closed),
        Ok(()) | Err(kernel::SendError::Full) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use audio::AudioDriver;
    use crossbeam_channel::{Receiver, bounded, unbounded};
    use kernel::{
        AudioCmd,
        AudioEvent,
        DriverEvent,
        Message,
        Outbox,
        domain::{AudioSettings, DriverError, DriverName},
    };

    use crate::{
        driver::{DriverLoop, DriverThread, spawn_driver, spawn_idle},
        jobs::Jobs,
        registry,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    fn start_audio_driver() -> (DriverThread<AudioCmd>, Receiver<Message>) {
        let (inbox, sent) = unbounded();
        let (deck_sender, heard) = bounded(64);
        let settings = AudioSettings::default();
        let row = registry::row(DriverName::Audio);
        let jobs = Jobs {
            pick: audio::EngineEffect::into_job,
            run: audio::AudioJob::run,
        };
        let thread = DriverLoop::<AudioDriver, _> {
            row,
            inbox,
            heard,
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
    #[ignore = "hardware: opens the output device"]
    fn a_closed_outbox_ends_the_audio_loop() {
        let (thread, sent) = start_audio_driver();
        drop(sent);

        thread.commands.send(AudioCmd::ListDevices).unwrap();

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
                let _ = inbox.recv();
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
