use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, SendError, Sender, unbounded};
use kernel::{
    DriverMessage,
    Message,
    domain::{Driver, DriverFailure},
};

use crate::error::RuntimeError;

pub(crate) type Delivery = Result<(), SendError<Message>>;

#[derive(Debug)]
pub(crate) struct DriverThread<C> {
    pub(crate) commands: Sender<C>,
    pub(crate) handle: JoinHandle<Delivery>,
}

pub trait DriverLoop<C>: Send + 'static {
    fn run(self, inbox: &Receiver<C>, mailbox: &Sender<Message>);
}

#[derive(Debug, Default)]
pub struct NoDriver;

impl<C: Send + 'static> DriverLoop<C> for NoDriver {
    fn run(self, inbox: &Receiver<C>, _mailbox: &Sender<Message>) {
        while inbox.recv().is_ok() {}
    }
}

pub(crate) fn spawn_loop<C, L>(
    driver: Driver,
    driver_loop: L,
    mailbox: Sender<Message>,
) -> Result<DriverThread<C>, RuntimeError>
where
    C: Send + 'static,
    L: DriverLoop<C>,
{
    spawn_driver(
        driver,
        move |inbox, mailbox| driver_loop.run(inbox, mailbox),
        mailbox,
    )
}

pub(crate) fn spawn_driver<C, F>(
    driver: Driver,
    run: F,
    mailbox: Sender<Message>,
) -> Result<DriverThread<C>, RuntimeError>
where
    C: Send + 'static,
    F: FnOnce(&Receiver<C>, &Sender<Message>) + Send + 'static,
{
    let (commands, inbox) = unbounded::<C>();
    let handle = thread::Builder::new()
        .name(format!("sifr-{driver}"))
        .spawn(move || -> Delivery {
            let outcome = catch_unwind(AssertUnwindSafe(|| run(&inbox, &mailbox)));
            let report = match outcome {
                Ok(()) => DriverMessage::Stopped,
                Err(payload) => {
                    DriverMessage::Died(DriverFailure::Panicked(panic_text(&*payload)))
                }
            };
            mailbox.send(Message::Driver(driver, report))
        })
        .map_err(|source| RuntimeError::Spawn { driver, source })?;
    Ok(DriverThread { commands, handle })
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

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        DriverMessage,
        Message,
        domain::{Driver, DriverFailure},
    };

    use crate::driver::{NoDriver, spawn_driver, spawn_loop};

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn a_panicking_driver_becomes_died() {
        let (mailbox, reports) = unbounded();
        let thread = spawn_driver(
            Driver::Audio,
            |_inbox: &Receiver<()>, _mailbox: &Sender<Message>| panic!("boom"),
            mailbox,
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
            Driver::Library,
            |_inbox: &Receiver<()>, _mailbox: &Sender<Message>| {},
            mailbox,
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
            Driver::Macos,
            |inbox: &Receiver<()>, _mailbox: &Sender<Message>| {
                let _ = inbox.recv();
            },
            mailbox,
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
        let thread =
            spawn_loop::<(), NoDriver>(Driver::Audio, NoDriver, mailbox).unwrap();
        drop(thread.commands);

        thread.handle.join().unwrap().unwrap();
        let message = reports.recv_timeout(RECV_TIMEOUT).unwrap();

        assert_eq!(
            message,
            Message::Driver(Driver::Audio, DriverMessage::Stopped)
        );
    }
}
