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

use crate::{error::Error, registry::DriverRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SendError {
    #[error("the inbox is closed")]
    Closed,
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
    pub(crate) commands: Sender<C>,
    pub(crate) handle: JoinHandle<Result<(), SendError>>,
    pub(crate) full: Congestion,
}

const INBOX: usize = 64;

pub(crate) fn send(
    inbox: &Sender<Message>,
    full: &Congestion,
    message: Message,
) -> Result<(), SendError> {
    match inbox.try_send(message) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(message)) => {
            full.raise();
            match inbox.send(message) {
                Ok(()) => Ok(()),
                Err(_) => Err(SendError::Closed),
            }
        }
        Err(TrySendError::Disconnected(_)) => Err(SendError::Closed),
    }
}

#[cfg(test)]
pub(crate) fn spawn_idle<C: Send + 'static>(
    row: &DriverRow,
    inbox: &Sender<Message>,
) -> Result<DriverThread<C>, Error> {
    spawn_driver(
        row,
        |inbox: &Receiver<C>, _: &Sender<Message>, _: &Congestion| {
            while inbox.recv().is_ok() {}
        },
        inbox,
    )
}

pub(crate) fn spawn_driver<C, R>(
    row: &DriverRow,
    run: R,
    inbox: &Sender<Message>,
) -> Result<DriverThread<C>, Error>
where
    C: Send + 'static,
    R: FnOnce(&Receiver<C>, &Sender<Message>, &Congestion) + Send + 'static,
{
    let (commands, command_inbox): (Sender<C>, Receiver<C>) = bounded(INBOX);
    let full = Congestion::default();
    let inbox = inbox.clone();
    let driver = row.driver;
    let handle = thread::Builder::new()
        .name(row.thread_name.to_owned())
        .spawn({
            let full = full.clone();
            move || -> Result<(), SendError> {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run(&command_inbox, &inbox, &full);
                }));
                let report = match result {
                    Ok(()) => DriverEvent::Stopped,
                    Err(_payload) => DriverEvent::Died(DriverError::Panicked),
                };
                send(
                    &inbox,
                    &full,
                    Message::Driver {
                        driver,
                        event: report,
                    },
                )
            }
        })
        .map_err(|source| Error::Spawn { driver, source })?;
    Ok(DriverThread {
        commands,
        handle,
        full,
    })
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use kernel::{
        domain::driver::{DriverError, DriverName},
        message::{AudioEvent, DriverEvent, Message},
    };
    use rstest::rstest;

    use crate::{
        driver_thread::{Congestion, SendError, send, spawn_driver, spawn_idle},
        registry,
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
    fn a_send_reports_its_outcome(
        #[case] scenario: Scenario,
        #[case] expected: Result<(), SendError>,
    ) {
        let (inbox, receiver) = bounded(1);
        let full = Congestion::default();
        let drainer = match scenario {
            Scenario::Room => None,
            Scenario::ReceiverDropped => {
                drop(receiver);
                None
            }
            Scenario::Full => {
                assert_eq!(
                    send(&inbox, &full, AudioEvent::TrackChanged.into()),
                    Ok(())
                );
                Some(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(20));
                    receiver.recv().unwrap();
                    receiver.recv().unwrap();
                }))
            }
        };

        let delivery = send(&inbox, &full, AudioEvent::TrackChanged.into());

        assert_eq!(delivery, expected);
        assert_eq!(full.take(), matches!(scenario, Scenario::Full));
        if let Some(drainer) = drainer {
            drainer.join().unwrap();
        }
    }

    #[test]
    fn a_panicking_driver_becomes_died() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(DriverName::Audio),
            |_inbox: &Receiver<()>, _: &Sender<Message>, _: &Congestion| panic!("boom"),
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
            |_inbox: &Receiver<()>, _: &Sender<Message>, _: &Congestion| {},
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
            |inbox: &Receiver<()>, _: &Sender<Message>, _: &Congestion| {
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
