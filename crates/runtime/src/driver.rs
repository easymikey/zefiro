use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, SendError, Sender, bounded};
use kernel::{DriverMessage, Message, domain::DriverError};

use crate::{
    error::Error,
    registry::DriverRow,
    sender::{DriverSender, FullEdge},
};

pub(crate) type Exit = Result<(), SendError<Message>>;

#[derive(Debug)]
pub(crate) struct DriverThread<C> {
    pub(crate) commands: Sender<C>,
    pub(crate) handle: JoinHandle<Exit>,
    pub(crate) full_edge: FullEdge,
}

const INBOX: usize = 64;

pub(crate) fn spawn_idle<C: Send + 'static>(
    row: &DriverRow,
    inbox: &Sender<Message>,
) -> Result<DriverThread<C>, Error> {
    spawn_driver(
        row,
        |inbox: &Receiver<C>, _: &DriverSender<Message>| while inbox.recv().is_ok() {},
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
    R: FnOnce(&Receiver<C>, &DriverSender<F>) + Send + 'static,
{
    let (commands, command_inbox): (Sender<C>, Receiver<C>) = bounded(INBOX);
    let full_edge = FullEdge::default();
    let outbox = DriverSender::new(inbox.clone(), full_edge.clone());
    let report_sender = inbox.clone();
    let driver = row.driver;
    let handle = thread::Builder::new()
        .name(row.thread_name.to_owned())
        .spawn(move || -> Exit {
            let outcome =
                catch_unwind(AssertUnwindSafe(|| run(&command_inbox, &outbox)));
            let report = match outcome {
                Ok(()) => DriverMessage::Stopped,
                Err(payload) => {
                    DriverMessage::Died(DriverError::Panicked(panic_text(&*payload)))
                }
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

fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::{Receiver, unbounded};
    use kernel::{
        DriverMessage,
        Message,
        domain::{Driver, DriverError},
    };

    use crate::{
        driver::{spawn_driver, spawn_idle},
        registry,
        sender::DriverSender,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn a_panicking_driver_becomes_died() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(Driver::Audio),
            |_inbox: &Receiver<()>, _outbox: &DriverSender<Message>| panic!("boom"),
            &inbox,
        )
        .unwrap();

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: Driver::Audio,
                event: DriverMessage::Died(DriverError::Panicked("boom".to_owned()))
            }
        );
    }

    #[test]
    fn a_returning_driver_becomes_stopped() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(Driver::Library),
            |_inbox: &Receiver<()>, _outbox: &DriverSender<Message>| {},
            &inbox,
        )
        .unwrap();

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: Driver::Library,
                event: DriverMessage::Stopped
            }
        );
    }

    #[test]
    fn a_closed_inbox_stops_the_driver() {
        let (inbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(Driver::Macos),
            |inbox: &Receiver<()>, _outbox: &DriverSender<Message>| {
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
                driver: Driver::Macos,
                event: DriverMessage::Stopped
            }
        );
    }

    #[test]
    fn an_idle_driver_stops_once_its_inbox_closes() {
        let (inbox, reports) = unbounded();
        let thread = spawn_idle::<()>(registry::row(Driver::Audio), &inbox).unwrap();
        drop(thread.commands);

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver {
                driver: Driver::Audio,
                event: DriverMessage::Stopped
            }
        );
    }
}
