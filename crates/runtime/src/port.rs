use std::{
    mem,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    cmd::{AudioCmd, ConfigCmd, LibraryCmd, MacosCmd},
    domain::driver::{DriverName, DriverStatus, Drivers},
    message::DriverEvent,
};

use crate::driver_thread::{Congestion, DriverThread, SendError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropReason {
    NotRunning,
    Full,
    Closed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Episode {
    #[default]
    Clear,
    Reported,
}

#[derive(Debug)]
pub(crate) enum Port<C> {
    Open {
        driver_name: DriverName,
        congestion: Congestion,
        episode: Episode,
        cmd_sender: Sender<C>,
        thread: JoinHandle<Result<(), SendError>>,
    },
    HungUp(JoinHandle<Result<(), SendError>>),
    Closed,
}

impl<C> Port<C> {
    pub(crate) fn spawned(driver_name: DriverName, thread: DriverThread<C>) -> Self {
        Self::Open {
            driver_name,
            congestion: thread.congestion,
            episode: Episode::Clear,
            cmd_sender: thread.cmd_sender,
            thread: thread.handle,
        }
    }

    pub(crate) fn congestion(&mut self) -> Option<DriverEvent> {
        let Self::Open {
            congestion,
            episode,
            ..
        } = self
        else {
            return None;
        };
        match (congestion.take(), *episode) {
            (true, Episode::Clear) => {
                *episode = Episode::Reported;
                Some(DriverEvent::Full)
            }
            (true, Episode::Reported) => None,
            (false, _) => {
                *episode = Episode::Clear;
                None
            }
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

    pub(crate) fn send(&self, drivers: &Drivers, cmd: C) -> Result<(), DropReason> {
        let Self::Open {
            driver_name,
            congestion,
            cmd_sender,
            ..
        } = self
        else {
            return Err(DropReason::Closed);
        };
        if !matches!(drivers.status(*driver_name), DriverStatus::Running) {
            return Err(DropReason::NotRunning);
        }
        cmd_sender.try_send(cmd).map_err(|error| match error {
            TrySendError::Full(_) => {
                congestion.raise();
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
    pub(crate) fn congestion(
        &mut self,
        driver_name: DriverName,
    ) -> Option<DriverEvent> {
        match driver_name {
            DriverName::Audio => self.audio.congestion(),
            DriverName::Library => self.library.congestion(),
            DriverName::Config => self.config.congestion(),
            DriverName::Macos => self.macos.congestion(),
        }
    }

    pub(crate) fn hang_up(&mut self, driver_name: DriverName) {
        match driver_name {
            DriverName::Audio => self.audio.hang_up(),
            DriverName::Library => self.library.hang_up(),
            DriverName::Config => self.config.hang_up(),
            DriverName::Macos => self.macos.hang_up(),
        }
    }

    pub(crate) fn join(
        &mut self,
        driver_name: DriverName,
    ) -> Option<thread::Result<Result<(), SendError>>> {
        match driver_name {
            DriverName::Audio => self.audio.join(),
            DriverName::Library => self.library.join(),
            DriverName::Config => self.config.join(),
            DriverName::Macos => self.macos.join(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use crossbeam_channel::{Sender, bounded, unbounded};
    use kernel::{
        cmd::AudioCmd,
        domain::driver::{DriverName, DriverStatus, Drivers},
    };
    use rstest::rstest;

    use crate::{
        driver_thread::Congestion,
        port::{DropReason, Episode, Port},
    };

    impl<C> Port<C> {
        pub(crate) fn new(
            driver_name: DriverName,
            cmd_sender: Sender<C>,
            congestion: Congestion,
        ) -> Self {
            Self::Open {
                driver_name,
                congestion,
                episode: Episode::Clear,
                cmd_sender,
                thread: thread::spawn(|| Ok(())),
            }
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
        let (cmd_sender, cmd_receiver) = unbounded();
        if let Inbox::Dropped = inbox {
            drop(cmd_receiver);
        }
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = status;
        let port = Port::new(DriverName::Audio, cmd_sender, Congestion::default());

        let sent = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(sent, expected);
    }

    #[test]
    fn a_full_cmd_receiver_is_dropped_and_raises_the_flag() {
        let (cmd_sender, _cmd_receiver) = bounded(1);
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = DriverStatus::Running;
        let congestion = Congestion::default();
        let port = Port::new(DriverName::Audio, cmd_sender, congestion.clone());

        port.send(&drivers, AudioCmd::Stop).unwrap();
        let second = port.send(&drivers, AudioCmd::Stop);

        assert_eq!(second, Err(DropReason::Full));
        assert!(congestion.take());
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
        let (cmd_sender, cmd_receiver) = unbounded::<AudioCmd>();
        let mut port = Port::new(DriverName::Audio, cmd_sender, Congestion::default());
        let mut last_join = None;

        for step in steps {
            match step {
                Step::HangUp => port.hang_up(),
                Step::Join => last_join = Some(port.join()),
            }
        }

        assert_eq!(stage(&port), expected);
        assert_eq!(last_join.is_some_and(|exit| exit.is_some()), joined_last);
        drop(cmd_receiver);
    }

    #[test]
    fn a_hung_up_port_refuses_every_cmd() {
        let (cmd_sender, _cmd_receiver) = unbounded();
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = DriverStatus::Running;
        let congestion = Congestion::default();
        let mut port = Port::new(DriverName::Audio, cmd_sender, congestion.clone());
        congestion.raise();

        port.hang_up();

        assert_eq!(port.send(&drivers, AudioCmd::Stop), Err(DropReason::Closed));
        assert!(port.congestion().is_none());
    }
}
