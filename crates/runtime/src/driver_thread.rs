use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use kernel::{
    domain::driver::DriverError,
    message::{DriverEvent, Message},
};

use crate::{error::SpawnError, registry::DriverRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SendError {
    #[error("the inbox is closed")]
    Closed,
}

#[derive(Debug)]
pub(crate) enum Halt {
    Inbox(SendError),
    Spawn(SpawnError),
}

impl From<SendError> for Halt {
    fn from(error: SendError) -> Self {
        Halt::Inbox(error)
    }
}

impl From<SpawnError> for Halt {
    fn from(error: SpawnError) -> Self {
        Halt::Spawn(error)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Congestion(Arc<AtomicBool>);

impl Congestion {
    pub(crate) fn raise(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub(crate) fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

#[derive(Debug)]
pub(crate) struct DriverThread<C> {
    pub(crate) cmd_sender: Sender<C>,
    pub(crate) handle: JoinHandle<()>,
    pub(crate) congestion: Congestion,
}

const INBOX: usize = 64;

pub(crate) fn send(
    inbox: &Sender<Message>,
    congestion: &Congestion,
    message: Message,
) -> Result<(), SendError> {
    match inbox.try_send(message) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(message)) => {
            congestion.raise();
            match inbox.send(message) {
                Ok(()) => Ok(()),
                Err(_) => Err(SendError::Closed),
            }
        }
        Err(TrySendError::Disconnected(_)) => Err(SendError::Closed),
    }
}

pub(crate) fn spawn_driver<C, R>(
    row: &DriverRow,
    run: R,
    inbox: &Sender<Message>,
) -> Result<DriverThread<C>, SpawnError>
where
    C: Send + 'static,
    R: FnOnce(&Receiver<C>, &Sender<Message>, &Congestion) -> Result<(), DriverError>
        + Send
        + 'static,
{
    let (cmd_sender, cmd_receiver): (Sender<C>, Receiver<C>) = bounded(INBOX);
    let congestion = Congestion::default();
    let inbox = inbox.clone();
    let driver = row.driver_name;
    let handle = thread::Builder::new()
        .name(row.thread_name.to_owned())
        .spawn({
            let congestion = congestion.clone();
            move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run(&cmd_receiver, &inbox, &congestion)
                }));
                let report = match result {
                    Ok(Ok(())) => DriverEvent::Stopped,
                    Ok(Err(error)) => DriverEvent::Died(error),
                    Err(_payload) => DriverEvent::Died(DriverError::Panicked),
                };
                match send(
                    &inbox,
                    &congestion,
                    Message::Driver {
                        driver_name: driver,
                        event: report,
                    },
                ) {
                    Ok(()) | Err(SendError::Closed) => {}
                }
            }
        })
        .map_err(|error| SpawnError::Thread {
            driver_name: driver,
            error,
        })?;
    Ok(DriverThread {
        cmd_sender,
        handle,
        congestion,
    })
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use kernel::{
        domain::{
            driver::{DriverError, DriverName},
            io_error::IoError,
        },
        message::{AudioEvent, DriverEvent, Message},
    };
    use rstest::rstest;

    use crate::{
        driver_thread::{Congestion, SendError, send, spawn_driver},
        registry,
        spawn::tests::spawn_idle,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    #[derive(Clone, Copy)]
    enum Scenario {
        Room,
        ReceiverDropped,
        Full,
    }

    #[rstest]
    #[case::room(Scenario::Room, Ok(()))]
    #[case::receiver_dropped(Scenario::ReceiverDropped, Err(SendError::Closed))]
    #[case::full(Scenario::Full, Ok(()))]
    fn a_send_reports_its_result(
        #[case] scenario: Scenario,
        #[case] expected: Result<(), SendError>,
    ) {
        let (inbox, receiver) = bounded(1);
        let congestion = Congestion::default();
        let drainer = match scenario {
            Scenario::Room => None,
            Scenario::ReceiverDropped => {
                drop(receiver);
                None
            }
            Scenario::Full => {
                assert_eq!(
                    send(&inbox, &congestion, AudioEvent::TrackChanged.into()),
                    Ok(())
                );
                Some(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(20));
                    receiver.recv().unwrap();
                    receiver.recv().unwrap();
                }))
            }
        };

        let delivery = send(&inbox, &congestion, AudioEvent::TrackChanged.into());

        assert_eq!(delivery, expected);
        assert_eq!(congestion.take(), matches!(scenario, Scenario::Full));
        if let Some(drainer) = drainer {
            drainer.join().unwrap();
        }
    }

    #[rstest]
    #[case::panicking(
        |_: &Receiver<()>, _: &Sender<Message>, _: &Congestion| panic!("boom"),
        DriverEvent::Died(DriverError::Panicked)
    )]
    #[case::failing(
        |_: &Receiver<()>, _: &Sender<Message>, _: &Congestion| Err(DriverError::Spawn {
            error: IoError::Other
        }),
        DriverEvent::Died(DriverError::Spawn {
            error: IoError::Other
        })
    )]
    #[case::returning(
        |_: &Receiver<()>, _: &Sender<Message>, _: &Congestion| Ok(()),
        DriverEvent::Stopped
    )]
    #[case::closed_cmd_receiver(
        |cmd_receiver: &Receiver<()>, _: &Sender<Message>, _: &Congestion| {
            assert!(cmd_receiver.recv().is_err());
            Ok(())
        },
        DriverEvent::Stopped
    )]
    fn a_finished_driver_reports_how_it_ended(
        #[case] run: impl FnOnce(
            &Receiver<()>,
            &Sender<Message>,
            &Congestion,
        ) -> Result<(), DriverError>
        + Send
        + 'static,
        #[case] expected: DriverEvent,
    ) {
        let (inbox, reports) = unbounded();
        let thread =
            spawn_driver(registry::row(DriverName::Audio), run, &inbox).unwrap();
        drop(thread.cmd_sender);

        thread.handle.join().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver_name: DriverName::Audio,
                event: expected
            }
        );
    }

    #[test]
    fn an_idle_driver_stops_once_its_cmd_receiver_closes() {
        let (inbox, reports) = unbounded();
        let thread =
            spawn_idle::<()>(registry::row(DriverName::Audio), &inbox).unwrap();
        drop(thread.cmd_sender);

        thread.handle.join().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver_name: DriverName::Audio,
                event: DriverEvent::Stopped
            }
        );
    }
}
