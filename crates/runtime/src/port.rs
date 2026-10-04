use std::{
    cell::Cell,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    AudioCmd,
    ConfigCmd,
    Congestion,
    LibraryCmd,
    MacosCmd,
    domain::{DriverName, DriverStatus, Drivers},
};

use crate::{
    driver::{DriverThread, Exit},
    library::{cover::CoverRequest, machine::LibraryMessage},
    trace::{DropReason, TraceEntry},
};

#[derive(Debug)]
pub(crate) struct Port<C> {
    driver: DriverName,
    sender: Option<Sender<C>>,
    full_edge: Congestion,
    handle: Option<JoinHandle<Exit>>,
}

impl<C> Port<C> {
    pub(crate) fn new(
        driver: DriverName,
        sender: Sender<C>,
        full_edge: Congestion,
    ) -> Self {
        Self {
            driver,
            sender: Some(sender),
            full_edge,
            handle: None,
        }
    }

    pub(crate) fn spawned(driver: DriverName, thread: DriverThread<C>) -> Self {
        Self {
            handle: Some(thread.handle),
            ..Self::new(driver, thread.commands, thread.full_edge)
        }
    }

    pub(crate) fn full_edge(&self) -> &Congestion {
        &self.full_edge
    }

    pub(crate) fn hang_up(&mut self) {
        self.sender = None;
    }

    pub(crate) fn join(&mut self) -> Option<thread::Result<Exit>> {
        self.handle.take().map(JoinHandle::join)
    }
}

impl<C> Port<C>
where
    for<'a> &'a C: Into<&'static str>,
{
    pub(crate) fn send(&self, drivers: &Drivers, command: C) -> Result<(), TraceEntry> {
        let command_label: &'static str = (&command).into();
        let dropped = |reason| TraceEntry::Dropped {
            driver: self.driver,
            command: command_label,
            reason,
        };
        if !matches!(drivers.status(self.driver), DriverStatus::Running) {
            return Err(dropped(DropReason::NotRunning));
        }
        let Some(sender) = &self.sender else {
            return Err(dropped(DropReason::Closed));
        };
        sender.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => {
                self.full_edge.raise();
                dropped(DropReason::Full)
            }
            TrySendError::Disconnected(_) => dropped(DropReason::Closed),
        })
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

    pub(crate) fn send_command(
        &self,
        drivers: &Drivers,
        command: LibraryCmd,
    ) -> Result<(), TraceEntry> {
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
            | LibraryCmd::LoadHistory(..)
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
    ) -> Result<(), TraceEntry> {
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
    pub(crate) fn full_edge(&self, driver: DriverName) -> &Congestion {
        match driver {
            DriverName::Audio => self.audio.full_edge(),
            DriverName::Library => self.library.port.full_edge(),
            DriverName::Config => self.config.full_edge(),
            DriverName::Macos => self.macos.full_edge(),
        }
    }

    pub(crate) fn hang_up(&mut self) {
        self.audio.hang_up();
        self.macos.hang_up();
        self.library.port.hang_up();
        self.config.hang_up();
    }

    pub(crate) fn join(&mut self, driver: DriverName) -> Option<thread::Result<Exit>> {
        match driver {
            DriverName::Audio => self.audio.join(),
            DriverName::Library => self.library.port.join(),
            DriverName::Config => self.config.join(),
            DriverName::Macos => self.macos.join(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::{bounded, unbounded};
    use kernel::{
        AudioCmd,
        Congestion,
        domain::{DriverName, DriverStatus, Drivers},
    };
    use rstest::rstest;

    use crate::{
        port::Port,
        trace::{DropReason, TraceEntry},
    };

    fn dropped(reason: DropReason) -> TraceEntry {
        TraceEntry::Dropped {
            driver: DriverName::Audio,
            command: (&AudioCmd::Stop).into(),
            reason,
        }
    }

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
        DriverStatus::Dead(kernel::domain::DriverError::Panicked),
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
        drivers.record_mut(DriverName::Audio).status = status;
        let port = Port::new(DriverName::Audio, sender, Congestion::default());

        let sent = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(sent, expected.map_err(dropped));
    }

    #[test]
    fn a_full_inbox_is_dropped_and_raises_the_flag() {
        let (sender, _receiver) = bounded(1);
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = DriverStatus::Running;
        let full_edge = Congestion::default();
        let port = Port::new(DriverName::Audio, sender, full_edge.clone());

        port.send(&drivers, AudioCmd::Stop).unwrap();
        let second = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(second, Err(dropped(DropReason::Full)));
        assert!(full_edge.take());
    }
}
