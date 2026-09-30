use std::cell::Cell;

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    AudioCmd,
    ConfigCmd,
    LibraryCmd,
    MacosCmd,
    domain::{Driver, DriverStatus, Drivers},
};

use crate::{
    library::{cover::CoverRequest, machine::LibraryMessage},
    sender::FullEdge,
    trace::{DropReason, TraceEntry},
};

#[derive(Debug)]
pub(crate) struct Port<C> {
    driver: Driver,
    sender: Sender<C>,
    full_edge: FullEdge,
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
    pub(crate) fn new(driver: Driver, sender: Sender<C>, full_edge: FullEdge) -> Self {
        Self {
            driver,
            sender,
            full_edge,
        }
    }

    pub(crate) fn full_edge(&self) -> &FullEdge {
        &self.full_edge
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
                self.full_edge.raise();
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
    port: Port<LibraryMessage>,
    side: Cell<Option<u32>>,
}

impl LibraryPort {
    pub(crate) fn new(port: Port<LibraryMessage>) -> Self {
        Self {
            port,
            side: Cell::new(None),
        }
    }

    pub(crate) fn full_edge(&self) -> &FullEdge {
        self.port.full_edge()
    }

    pub(crate) fn send_command(
        &self,
        drivers: &Drivers,
        command: LibraryCmd,
    ) -> Result<(), Undelivered> {
        match command {
            LibraryCmd::PrefetchCover(path) => self.side.get().map_or(Ok(()), |side| {
                self.port.send(
                    drivers,
                    LibraryMessage::Cover(CoverRequest {
                        path,
                        size_px: side,
                    }),
                )
            }),
            other @ (LibraryCmd::AppendHistory { .. }
            | LibraryCmd::SaveFavorites(_)
            | LibraryCmd::LoadFavorites
            | LibraryCmd::Trash(_)
            | LibraryCmd::LoadHistory { .. }
            | LibraryCmd::Scan { .. }
            | LibraryCmd::SavePlaylist { .. }
            | LibraryCmd::TagTracks { .. }) => {
                self.port.send(drivers, LibraryMessage::Cmd(other))
            }
        }
    }

    pub(crate) fn send_cover(
        &self,
        drivers: &Drivers,
        request: CoverRequest,
    ) -> Result<(), Undelivered> {
        self.side.set(Some(request.size_px));
        self.port.send(drivers, LibraryMessage::Cover(request))
    }
}

#[derive(Debug)]
pub(crate) struct Ports {
    pub(crate) audio: Port<AudioCmd>,
    pub(crate) library: LibraryPort,
    pub(crate) config: Port<ConfigCmd>,
    pub(crate) macos: Port<MacosCmd>,
}

impl Ports {
    pub(crate) fn full_edge(&self, driver: Driver) -> Option<&FullEdge> {
        match driver {
            Driver::Audio => Some(self.audio.full_edge()),
            Driver::Library => Some(self.library.full_edge()),
            Driver::Config => Some(self.config.full_edge()),
            Driver::Macos => Some(self.macos.full_edge()),
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

    use crate::{port::Port, sender::FullEdge, trace::DropReason};

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
        DriverStatus::Dead(kernel::domain::DriverError::Panicked("boom".to_owned())),
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
        let port = Port::new(Driver::Audio, sender, FullEdge::default());

        let sent = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(sent.map_err(|undelivered| undelivered.reason), expected);
    }

    #[test]
    fn a_full_inbox_is_dropped_and_raises_the_flag() {
        let (sender, _receiver) = bounded(1);
        let mut drivers = Drivers::default();
        drivers.record_mut(Driver::Audio).status = DriverStatus::Running;
        let full_edge = FullEdge::default();
        let port = Port::new(Driver::Audio, sender, full_edge.clone());

        port.send(&drivers, AudioCmd::Stop).unwrap();
        let second = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(
            second.map_err(|undelivered| undelivered.reason),
            Err(DropReason::Full)
        );
        assert!(full_edge.take());
    }
}
