use std::{
    mem,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    cmd::{AudioCmd, ConfigCmd, LibraryCmd, MacosCmd},
    domain::driver::{DriverName, DriverStatus, Drivers},
};

use crate::driver_thread::{Congestion, DriverThread, SendError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropReason {
    NotRunning,
    Full,
    Closed,
}

#[derive(Debug)]
pub(crate) enum Port<C> {
    Open {
        driver: DriverName,
        full: Congestion,
        sender: Sender<C>,
        thread: JoinHandle<Result<(), SendError>>,
    },
    HungUp(JoinHandle<Result<(), SendError>>),
    Closed,
}

impl<C> Port<C> {
    #[cfg(test)]
    pub(crate) fn new(driver: DriverName, sender: Sender<C>, full: Congestion) -> Self {
        Self::Open {
            driver,
            full,
            sender,
            thread: thread::spawn(|| Ok(())),
        }
    }

    pub(crate) fn spawned(driver: DriverName, thread: DriverThread<C>) -> Self {
        Self::Open {
            driver,
            full: thread.full,
            sender: thread.commands,
            thread: thread.handle,
        }
    }

    pub(crate) fn full(&self) -> Option<&Congestion> {
        match self {
            Self::Open { full, .. } => Some(full),
            Self::HungUp(_) | Self::Closed => None,
        }
    }

    pub(crate) fn hang_up(&mut self) {
        *self = match mem::replace(self, Self::Closed) {
            Self::Open { thread, .. } => Self::HungUp(thread),
            port @ (Self::HungUp(_) | Self::Closed) => port,
        };
    }

    pub(crate) fn join(&mut self) -> Option<thread::Result<Result<(), SendError>>> {
        match mem::replace(self, Self::Closed) {
            Self::HungUp(thread) => Some(thread.join()),
            port @ (Self::Open { .. } | Self::Closed) => {
                *self = port;
                None
            }
        }
    }

    pub(crate) fn send(&self, drivers: &Drivers, command: C) -> Result<(), DropReason> {
        let Self::Open {
            driver,
            full,
            sender,
            ..
        } = self
        else {
            return Err(DropReason::Closed);
        };
        if !matches!(drivers.status(*driver), DriverStatus::Running) {
            return Err(DropReason::NotRunning);
        }
        sender.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => {
                full.raise();
                DropReason::Full
            }
            TrySendError::Disconnected(_) => DropReason::Closed,
        })
    }
}

#[derive(Debug)]
pub(crate) struct Ports {
    pub(crate) audio: Port<AudioCmd>,
    pub(crate) library: Port<LibraryCmd>,
    pub(crate) config: Port<ConfigCmd>,
    pub(crate) macos: Port<MacosCmd>,
}

impl Ports {
    pub(crate) fn full(&self, driver: DriverName) -> Option<&Congestion> {
        match driver {
            DriverName::Audio => self.audio.full(),
            DriverName::Library => self.library.full(),
            DriverName::Config => self.config.full(),
            DriverName::Macos => self.macos.full(),
        }
    }

    pub(crate) fn hang_up(&mut self, driver: DriverName) {
        match driver {
            DriverName::Audio => self.audio.hang_up(),
            DriverName::Library => self.library.hang_up(),
            DriverName::Config => self.config.hang_up(),
            DriverName::Macos => self.macos.hang_up(),
        }
    }

    pub(crate) fn join(
        &mut self,
        driver: DriverName,
    ) -> Option<thread::Result<Result<(), SendError>>> {
        match driver {
            DriverName::Audio => self.audio.join(),
            DriverName::Library => self.library.join(),
            DriverName::Config => self.config.join(),
            DriverName::Macos => self.macos.join(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::{bounded, unbounded};
    use kernel::{
        cmd::AudioCmd,
        domain::driver::{DriverName, DriverStatus, Drivers},
    };
    use rstest::rstest;

    use crate::{
        driver_thread::Congestion,
        port::{DropReason, Port},
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
        DriverStatus::Dead(kernel::domain::driver::DriverError::Panicked),
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

        assert_eq!(sent, expected);
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

        assert_eq!(second, Err(DropReason::Full));
        assert!(full.take());
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Stage {
        Open,
        HungUp,
        Closed,
    }

    fn stage<C>(port: &Port<C>) -> Stage {
        match port {
            Port::Open { .. } => Stage::Open,
            Port::HungUp(_) => Stage::HungUp,
            Port::Closed => Stage::Closed,
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Step {
        HangUp,
        Join,
    }

    #[rstest]
    #[case::open_stays_open_when_joined(&[Step::Join], Stage::Open, false)]
    #[case::open_hangs_up(&[Step::HangUp], Stage::HungUp, false)]
    #[case::hung_up_joins_into_closed(&[Step::HangUp, Step::Join], Stage::Closed, true)]
    #[case::hanging_up_twice_is_one_hang_up(
        &[Step::HangUp, Step::HangUp],
        Stage::HungUp,
        false
    )]
    #[case::a_joined_port_has_nothing_to_join(
        &[Step::HangUp, Step::Join, Step::Join],
        Stage::Closed,
        false
    )]
    fn a_port_goes_from_open_to_hung_up_to_closed(
        #[case] steps: &[Step],
        #[case] expected: Stage,
        #[case] joined_last: bool,
    ) {
        let (sender, receiver) = unbounded::<AudioCmd>();
        let mut port = Port::new(DriverName::Audio, sender, Congestion::default());
        let mut last_join = None;

        for step in steps {
            match step {
                Step::HangUp => port.hang_up(),
                Step::Join => last_join = Some(port.join()),
            }
        }

        assert_eq!(stage(&port), expected);
        assert_eq!(last_join.is_some_and(|exit| exit.is_some()), joined_last);
        drop(receiver);
    }

    #[test]
    fn a_hung_up_port_refuses_every_command() {
        let (sender, _receiver) = unbounded();
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = DriverStatus::Running;
        let mut port = Port::new(DriverName::Audio, sender, Congestion::default());

        port.hang_up();

        assert_eq!(port.send(&drivers, AudioCmd::Stop), Err(DropReason::Closed));
        assert!(port.full().is_none());
    }
}
