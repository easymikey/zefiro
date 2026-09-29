use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, SendError, Sender, bounded};
use kernel::{DriverMessage, Message, domain::DriverFailure};

use crate::{
    error::RuntimeError,
    mailbox::{Congestion, Mailbox},
    registry::DriverRow,
};

pub(crate) type Report = Result<(), SendError<Message>>;

#[derive(Debug)]
pub(crate) struct DriverThread<C> {
    pub(crate) commands: Sender<C>,
    pub(crate) handle: JoinHandle<Report>,
    pub(crate) congestion: Congestion,
}

pub(crate) trait DriverLoop<C, F>: Send + 'static {
    fn run(self, inbox: &Receiver<C>, outbox: &Mailbox<F>);
}

#[derive(Debug, Default)]
pub(crate) struct NoDriver;

impl<C: Send + 'static, F: Send + 'static> DriverLoop<C, F> for NoDriver {
    fn run(self, inbox: &Receiver<C>, _outbox: &Mailbox<F>) {
        while inbox.recv().is_ok() {}
    }
}

pub(crate) fn spawn_loop<C, F, L>(
    row: &DriverRow,
    driver_loop: L,
    mailbox: &Sender<Message>,
) -> Result<DriverThread<C>, RuntimeError>
where
    C: Send + 'static,
    F: Send + 'static,
    L: DriverLoop<C, F>,
{
    spawn_driver(
        row,
        move |inbox, outbox| driver_loop.run(inbox, outbox),
        mailbox,
    )
}

pub(crate) fn spawn_driver<C, F, R>(
    row: &DriverRow,
    run: R,
    mailbox: &Sender<Message>,
) -> Result<DriverThread<C>, RuntimeError>
where
    C: Send + 'static,
    F: Send + 'static,
    R: FnOnce(&Receiver<C>, &Mailbox<F>) + Send + 'static,
{
    let (commands, inbox): (Sender<C>, Receiver<C>) = bounded(row.inbox);
    let congestion = Congestion::default();
    let outbox = Mailbox::new(mailbox.clone(), congestion.clone());
    let report_sender = mailbox.clone();
    let driver = row.driver;
    let handle = thread::Builder::new()
        .name(row.thread.to_owned())
        .spawn(move || -> Report {
            let outcome = catch_unwind(AssertUnwindSafe(|| run(&inbox, &outbox)));
            let report = match outcome {
                Ok(()) => DriverMessage::Stopped,
                Err(payload) => {
                    DriverMessage::Died(DriverFailure::Panicked(panic_text(&*payload)))
                }
            };
            report_sender.send(Message::Driver(driver, report))
        })
        .map_err(|source| RuntimeError::Spawn { driver, source })?;
    Ok(DriverThread {
        commands,
        handle,
        congestion,
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
        domain::{Driver, DriverFailure},
    };

    use crate::{
        driver::{NoDriver, spawn_driver, spawn_loop},
        mailbox::Mailbox,
        registry,
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn a_panicking_driver_becomes_died() {
        let (mailbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(Driver::Audio),
            |_inbox: &Receiver<()>, _outbox: &Mailbox<Message>| panic!("boom"),
            &mailbox,
        )
        .unwrap();

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver(
                Driver::Audio,
                DriverMessage::Died(DriverFailure::Panicked("boom".to_owned()))
            )
        );
    }

    #[test]
    fn a_returning_driver_becomes_stopped() {
        let (mailbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(Driver::Library),
            |_inbox: &Receiver<()>, _outbox: &Mailbox<Message>| {},
            &mailbox,
        )
        .unwrap();

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver(Driver::Library, DriverMessage::Stopped)
        );
    }

    #[test]
    fn a_closed_inbox_stops_the_driver() {
        let (mailbox, reports) = unbounded();
        let thread = spawn_driver(
            registry::row(Driver::Macos),
            |inbox: &Receiver<()>, _outbox: &Mailbox<Message>| {
                let _ = inbox.recv();
            },
            &mailbox,
        )
        .unwrap();
        drop(thread.commands);

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver(Driver::Macos, DriverMessage::Stopped)
        );
    }

    #[test]
    fn a_driver_loop_stops_once_its_inbox_closes() {
        let (mailbox, reports) = unbounded();
        let thread = spawn_loop::<(), Message, NoDriver>(
            registry::row(Driver::Audio),
            NoDriver,
            &mailbox,
        )
        .unwrap();
        drop(thread.commands);

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver(Driver::Audio, DriverMessage::Stopped)
        );
    }
}
