use crossbeam_channel::Sender;
use kernel::{
    AudioCmd,
    LibraryCmd,
    SystemCmd,
    domain::{Driver, DriverStatus, Drivers},
};

use crate::{
    interpret::ConfigCommand,
    library::cover::CoverRequest,
    trace::TraceEntry,
};

#[derive(Debug)]
pub(crate) struct Port<C> {
    driver: Driver,
    sender: Sender<C>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Undelivered {
    pub(crate) driver: Driver,
    pub(crate) command: &'static str,
}

impl From<Undelivered> for TraceEntry {
    fn from(undelivered: Undelivered) -> Self {
        TraceEntry::Dropped {
            driver: undelivered.driver,
            command: undelivered.command,
        }
    }
}

impl<C> Port<C> {
    pub(crate) fn new(driver: Driver, sender: Sender<C>) -> Self {
        Self { driver, sender }
    }
}

impl<C> Port<C>
where
    for<'a> &'a C: Into<&'static str>,
{
    pub(crate) fn send(
        &self,
        drivers: &Drivers,
        command: C,
    ) -> Result<(), Undelivered> {
        let label: &'static str = (&command).into();
        let running = matches!(drivers.status(self.driver), DriverStatus::Running);
        if running && self.sender.send(command).is_ok() {
            Ok(())
        } else {
            Err(Undelivered {
                driver: self.driver,
                command: label,
            })
        }
    }
}

#[derive(Debug)]
pub(crate) struct Ports {
    pub(crate) audio: Port<AudioCmd>,
    pub(crate) library: Port<LibraryCmd>,
    pub(crate) covers: Port<CoverRequest>,
    pub(crate) config: Port<ConfigCommand>,
    pub(crate) macos: Option<Port<SystemCmd>>,
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;
    use kernel::{
        AudioCmd,
        domain::{Driver, DriverStatus, Drivers},
    };
    use rstest::rstest;

    use crate::port::Port;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Inbox {
        Connected,
        Dropped,
    }

    #[rstest]
    #[case::running_and_connected(DriverStatus::Running, Inbox::Connected, true)]
    #[case::stopped(DriverStatus::Stopped, Inbox::Connected, false)]
    #[case::dead(DriverStatus::Dead(kernel::domain::DriverFailure::Panicked("boom".to_owned())), Inbox::Connected, false)]
    #[case::running_and_disconnected(DriverStatus::Running, Inbox::Dropped, false)]
    fn a_port_sends_only_to_a_running_driver(
        #[case] status: DriverStatus,
        #[case] inbox: Inbox,
        #[case] expects_delivery: bool,
    ) {
        let (sender, receiver) = unbounded();
        if let Inbox::Dropped = inbox {
            drop(receiver);
        }
        let mut drivers = Drivers::default();
        drivers.record_mut(Driver::Audio).status = status;
        let port = Port::new(Driver::Audio, sender);

        let sent = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(sent.is_ok(), expects_delivery);
    }
}
