use std::thread::{self, JoinHandle};

use crossbeam_channel::{SendError, Sender, TrySendError};
use kernel::{
    AudioCmd,
    ConfigCmd,
    LibraryCmd,
    MacosCmd,
    Message,
    domain::{DriverName, DriverStatus, Drivers},
};
use library::{CoverJob, LibraryMessage};

use crate::{
    driver::DriverThread,
    outbox::Congestion,
    trace::{DropReason, TraceEntry},
};

#[derive(Debug)]
pub(crate) struct Port<C> {
    driver: DriverName,
    sender: Option<Sender<C>>,
    full: Congestion,
    handle: Option<JoinHandle<Result<(), SendError<Message>>>>,
}

impl<C> Port<C> {
    pub(crate) fn new(driver: DriverName, sender: Sender<C>, full: Congestion) -> Self {
        Self {
            driver,
            sender: Some(sender),
            full,
            handle: None,
        }
    }

    pub(crate) fn spawned(driver: DriverName, thread: DriverThread<C>) -> Self {
        Self {
            handle: Some(thread.handle),
            ..Self::new(driver, thread.commands, thread.full)
        }
    }

    pub(crate) fn full(&self) -> &Congestion {
        &self.full
    }

    pub(crate) fn hang_up(&mut self) {
        self.sender = None;
    }

    pub(crate) fn join(
        &mut self,
    ) -> Option<thread::Result<Result<(), SendError<Message>>>> {
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
        let sender = self.open(drivers).map_err(dropped)?;
        sender
            .try_send(command)
            .map_err(|error| dropped(self.refused(&error)))
    }
}

impl<C> Port<C> {
    fn open(&self, drivers: &Drivers) -> Result<&Sender<C>, DropReason> {
        if !matches!(drivers.status(self.driver), DriverStatus::Running) {
            return Err(DropReason::NotRunning);
        }
        self.sender.as_ref().ok_or(DropReason::Closed)
    }

    fn refused<T>(&self, error: &TrySendError<T>) -> DropReason {
        match error {
            TrySendError::Full(_) => {
                self.full.raise();
                DropReason::Full
            }
            TrySendError::Disconnected(_) => DropReason::Closed,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LibraryPort {
    port: Port<LibraryCmd>,
    covers: Sender<LibraryMessage>,
}

impl LibraryPort {
    pub(crate) fn new(port: Port<LibraryCmd>, covers: Sender<LibraryMessage>) -> Self {
        Self { port, covers }
    }

    pub(crate) fn spawned(
        (thread, covers): (DriverThread<LibraryCmd>, Sender<LibraryMessage>),
    ) -> Self {
        Self::new(Port::spawned(DriverName::Library, thread), covers)
    }

    pub(crate) fn send_command(
        &self,
        drivers: &Drivers,
        command: LibraryCmd,
    ) -> Result<(), TraceEntry> {
        self.port.send(drivers, command)
    }

    pub(crate) fn send_cover(
        &self,
        drivers: &Drivers,
        job: CoverJob,
    ) -> Result<(), TraceEntry> {
        let dropped = |reason| TraceEntry::Dropped {
            driver: self.port.driver,
            command: "cover",
            reason,
        };
        self.port.open(drivers).map_err(dropped)?;
        self.covers
            .try_send(LibraryMessage::Cover(job))
            .map_err(|error| dropped(self.port.refused(&error)))
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
    pub(crate) fn full(&self, driver: DriverName) -> &Congestion {
        match driver {
            DriverName::Audio => self.audio.full(),
            DriverName::Library => self.library.port.full(),
            DriverName::Config => self.config.full(),
            DriverName::Macos => self.macos.full(),
        }
    }

    pub(crate) fn hang_up(&mut self) {
        self.audio.hang_up();
        self.macos.hang_up();
        self.library.port.hang_up();
        self.config.hang_up();
    }

    pub(crate) fn join(
        &mut self,
        driver: DriverName,
    ) -> Option<thread::Result<Result<(), SendError<Message>>>> {
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
        domain::{DriverName, DriverStatus, Drivers},
    };
    use rstest::rstest;

    use crate::{
        outbox::Congestion,
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
        let full = Congestion::default();
        let port = Port::new(DriverName::Audio, sender, full.clone());

        port.send(&drivers, AudioCmd::Stop).unwrap();
        let second = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(second, Err(dropped(DropReason::Full)));
        assert!(full.take());
    }
}
