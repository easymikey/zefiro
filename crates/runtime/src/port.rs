use std::cell::Cell;

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    AudioCmd,
    LibraryCmd,
    SystemCmd,
    domain::{Driver, DriverStatus, Drivers},
};

use crate::{
    interpret::{ConfigCommand, LibraryCommand},
    library::cover::CoverRequest,
    mailbox::Congestion,
    trace::{DropReason, TraceEntry},
};

#[derive(Debug)]
pub(crate) struct Port<C> {
    driver: Driver,
    sender: Sender<C>,
    congestion: Congestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Undelivered {
    pub(crate) driver: Driver,
    pub(crate) command: &'static str,
    pub(crate) reason: DropReason,
}

impl From<Undelivered> for TraceEntry {
    fn from(undelivered: Undelivered) -> Self {
        TraceEntry::Dropped {
            driver: undelivered.driver,
            command: undelivered.command,
            reason: undelivered.reason,
        }
    }
}

impl<C> Port<C> {
    pub(crate) fn new(
        driver: Driver,
        sender: Sender<C>,
        congestion: Congestion,
    ) -> Self {
        Self {
            driver,
            sender,
            congestion,
        }
    }

    pub(crate) fn congestion(&self) -> &Congestion {
        &self.congestion
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
        if !matches!(drivers.status(self.driver), DriverStatus::Running) {
            return Err(Undelivered {
                driver: self.driver,
                command: label,
                reason: DropReason::NotRunning,
            });
        }
        match self.sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.congestion.raise();
                Err(Undelivered {
                    driver: self.driver,
                    command: label,
                    reason: DropReason::Full,
                })
            }
            Err(TrySendError::Disconnected(_)) => Err(Undelivered {
                driver: self.driver,
                command: label,
                reason: DropReason::Closed,
            }),
        }
    }
}

#[derive(Debug)]
pub(crate) struct LibraryPort {
    port: Port<LibraryCommand>,
    side: Cell<Option<u32>>,
}

impl LibraryPort {
    pub(crate) fn new(port: Port<LibraryCommand>) -> Self {
        Self {
            port,
            side: Cell::new(None),
        }
    }

    pub(crate) fn congestion(&self) -> &Congestion {
        self.port.congestion()
    }

    pub(crate) fn send_command(
        &self,
        drivers: &Drivers,
        command: LibraryCmd,
    ) -> Result<(), Undelivered> {
        match command {
            LibraryCmd::PrefetchCover(path) => self.side.get().map_or(Ok(()), |side| {
                self.port
                    .send(drivers, LibraryCommand::Cover(CoverRequest { path, side }))
            }),
            other @ (LibraryCmd::AppendHistory { .. }
            | LibraryCmd::SaveFavorites(_)
            | LibraryCmd::LoadFavorites
            | LibraryCmd::Trash(_)
            | LibraryCmd::LoadHistory { .. }
            | LibraryCmd::Rescan { .. }
            | LibraryCmd::SavePlaylist { .. }
            | LibraryCmd::ScanLibrary { .. }
            | LibraryCmd::TagTracks { .. }) => {
                self.port.send(drivers, LibraryCommand::Kernel(other))
            }
        }
    }

    pub(crate) fn send_cover(
        &self,
        drivers: &Drivers,
        request: CoverRequest,
    ) -> Result<(), Undelivered> {
        self.side.set(Some(request.side));
        self.port.send(drivers, LibraryCommand::Cover(request))
    }
}

#[derive(Debug)]
pub(crate) struct Ports {
    pub(crate) audio: Port<AudioCmd>,
    pub(crate) library: LibraryPort,
    pub(crate) config: Port<ConfigCommand>,
    pub(crate) macos: Port<SystemCmd>,
}

impl Ports {
    pub(crate) fn congestion(&self, driver: Driver) -> Option<&Congestion> {
        match driver {
            Driver::Audio => Some(self.audio.congestion()),
            Driver::Library => Some(self.library.congestion()),
            Driver::Config => Some(self.config.congestion()),
            Driver::Macos => Some(self.macos.congestion()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::{bounded, unbounded};
    use kernel::{
        AudioCmd,
        domain::{Driver, DriverStatus, Drivers},
    };
    use rstest::rstest;

    use crate::{
        mailbox::{Congestion, Crowding},
        port::Port,
        trace::DropReason,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Inbox {
        Connected,
        Dropped,
    }

    #[rstest]
    #[case::running_and_connected(DriverStatus::Running, Inbox::Connected, Ok(()))]
    #[case::stopped(
        DriverStatus::Stopped,
        Inbox::Connected,
        Err(DropReason::NotRunning)
    )]
    #[case::dead(
        DriverStatus::Dead(kernel::domain::DriverFailure::Panicked("boom".to_owned())),
        Inbox::Connected,
        Err(DropReason::NotRunning)
    )]
    #[case::running_and_disconnected(
        DriverStatus::Running,
        Inbox::Dropped,
        Err(DropReason::Closed)
    )]
    fn a_port_sends_only_to_a_running_driver(
        #[case] status: DriverStatus,
        #[case] inbox: Inbox,
        #[case] expected: Result<(), DropReason>,
    ) {
        let (sender, receiver) = unbounded();
        if let Inbox::Dropped = inbox {
            drop(receiver);
        }
        let mut drivers = Drivers::default();
        drivers.record_mut(Driver::Audio).status = status;
        let port = Port::new(Driver::Audio, sender, Congestion::default());

        let sent = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(sent.map_err(|undelivered| undelivered.reason), expected);
    }

    #[test]
    fn a_full_inbox_is_dropped_and_raises_the_flag() {
        let (sender, _receiver) = bounded(1);
        let mut drivers = Drivers::default();
        drivers.record_mut(Driver::Audio).status = DriverStatus::Running;
        let congestion = Congestion::default();
        let port = Port::new(Driver::Audio, sender, congestion.clone());

        port.send(&drivers, AudioCmd::Stop).unwrap();
        let second = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(
            second.map_err(|undelivered| undelivered.reason),
            Err(DropReason::Full)
        );
        assert_eq!(congestion.settle(), Crowding::Crowded);
    }
}
