use std::{mem, thread::JoinHandle};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    cmd::{AudioCmd, ConfigCmd, LibraryCmd, MacosCmd, RemoteCmd},
    domain::driver::{DriverName, DriverStatus, Drivers},
    message::DriverEvent,
};

use crate::driver_thread::{Congestion, DriverThread};

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
        thread: JoinHandle<()>,
    },
    HungUp(JoinHandle<()>),
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
            driver_name: _driver_name,
            cmd_sender: _cmd_sender,
            thread: _thread,
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
            Self::Open {
                thread,
                driver_name: _driver_name,
                congestion: _congestion,
                episode: _episode,
                cmd_sender: _cmd_sender,
            } => Self::HungUp(thread),
            port @ (Self::HungUp(_) | Self::Closed) => port,
        };
    }

    pub(crate) fn join(&mut self) {
        match mem::replace(self, Self::Closed) {
            Self::HungUp(thread) => match thread.join() {
                Ok(()) | Err(_) => {}
            },
            port @ (Self::Open { .. } | Self::Closed) => *self = port,
        }
    }

    pub(crate) fn send(&self, drivers: &Drivers, cmd: C) {
        let Self::Open {
            driver_name,
            congestion,
            cmd_sender,
            episode: _episode,
            thread: _thread,
        } = self
        else {
            return;
        };
        if !matches!(drivers.status(*driver_name), DriverStatus::Running) {
            return;
        }
        match cmd_sender.try_send(cmd) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => congestion.raise(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Ports {
    pub(crate) audio: Port<AudioCmd>,
    pub(crate) library: Port<LibraryCmd>,
    pub(crate) config: Port<ConfigCmd>,
    pub(crate) macos: Port<MacosCmd>,
    pub(crate) remote: Port<RemoteCmd>,
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
            DriverName::Remote => self.remote.congestion(),
        }
    }

    pub(crate) fn hang_up(&mut self, driver_name: DriverName) {
        match driver_name {
            DriverName::Audio => self.audio.hang_up(),
            DriverName::Library => self.library.hang_up(),
            DriverName::Config => self.config.hang_up(),
            DriverName::Macos => self.macos.hang_up(),
            DriverName::Remote => self.remote.hang_up(),
        }
    }

    pub(crate) fn join(&mut self, driver_name: DriverName) {
        match driver_name {
            DriverName::Audio => self.audio.join(),
            DriverName::Library => self.library.join(),
            DriverName::Config => self.config.join(),
            DriverName::Macos => self.macos.join(),
            DriverName::Remote => self.remote.join(),
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
        port::{Episode, Port},
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
                thread: thread::spawn(|| {}),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Inbox {
        Connected,
        Dropped,
    }

    #[rstest]
    #[case::running_and_connected(
        DriverStatus::Running,
        Inbox::Connected,
        vec![AudioCmd::Stop]
    )]
    #[case::stopped(DriverStatus::Stopped, Inbox::Connected, Vec::new())]
    #[case::dead(
        DriverStatus::Dead(kernel::domain::driver::DriverError::Panicked),
        Inbox::Connected,
        Vec::new()
    )]
    #[case::running_and_disconnected(DriverStatus::Running, Inbox::Dropped, Vec::new())]
    fn a_port_sends_only_to_a_running_driver(
        #[case] status: DriverStatus,
        #[case] inbox: Inbox,
        #[case] expected: Vec<AudioCmd>,
    ) {
        let (cmd_sender, cmd_receiver) = unbounded();
        let cmd_receiver = match inbox {
            Inbox::Connected => Some(cmd_receiver),
            Inbox::Dropped => {
                drop(cmd_receiver);
                None
            }
        };
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = status;
        let congestion = Congestion::default();
        let port = Port::new(DriverName::Audio, cmd_sender, congestion.clone());

        port.send(&drivers, AudioCmd::Stop);

        let audio_cmds: Vec<AudioCmd> = cmd_receiver
            .map_or_else(Vec::new, |cmd_receiver| cmd_receiver.try_iter().collect());
        assert_eq!(audio_cmds, expected);
        assert!(!congestion.take());
    }

    #[test]
    fn a_full_cmd_receiver_is_dropped_and_raises_the_flag() {
        let (cmd_sender, cmd_receiver) = bounded(1);
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = DriverStatus::Running;
        let congestion = Congestion::default();
        let port = Port::new(DriverName::Audio, cmd_sender, congestion.clone());

        port.send(&drivers, AudioCmd::Stop);
        port.send(&drivers, AudioCmd::ListDevices);

        let audio_cmds: Vec<AudioCmd> = cmd_receiver.try_iter().collect();
        assert_eq!(audio_cmds, vec![AudioCmd::Stop]);
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
    #[case::open_stays_open_when_joined(&[Step::Join], Stage::Open)]
    #[case::open_hangs_up(&[Step::HangUp], Stage::HungUp)]
    #[case::hung_up_joins_into_closed(&[Step::HangUp, Step::Join], Stage::Closed)]
    #[case::hanging_up_twice_is_one_hang_up(&[Step::HangUp, Step::HangUp], Stage::HungUp)]
    #[case::a_joined_port_has_nothing_to_join(
        &[Step::HangUp, Step::Join, Step::Join],
        Stage::Closed
    )]
    fn a_port_goes_from_open_to_hung_up_to_closed(
        #[case] steps: &[Step],
        #[case] expected: Stage,
    ) {
        let (cmd_sender, cmd_receiver) = unbounded::<AudioCmd>();
        let mut port = Port::new(DriverName::Audio, cmd_sender, Congestion::default());

        for step in steps {
            match step {
                Step::HangUp => port.hang_up(),
                Step::Join => port.join(),
            }
        }

        assert_eq!(stage(&port), expected);
        drop(cmd_receiver);
    }

    #[test]
    fn a_hung_up_port_refuses_every_cmd() {
        let (cmd_sender, cmd_receiver) = unbounded();
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = DriverStatus::Running;
        let congestion = Congestion::default();
        let mut port = Port::new(DriverName::Audio, cmd_sender, congestion.clone());
        congestion.raise();

        port.hang_up();
        port.send(&drivers, AudioCmd::Stop);

        assert!(cmd_receiver.is_empty());
        assert!(port.congestion().is_none());
    }
}
