use std::time::{Duration, Instant};

use audio::tap::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    domain::{
        driver::{DriverError, DriverName, DriverStatus},
        model::Model,
        settings::AudioSettings,
    },
    message::{DriverEvent, Message},
};

#[cfg(target_os = "macos")] use crate::spawn_setup::MacosChannel;
use crate::{
    error::SpawnError,
    latest::{LatestReceivers, LatestSenders, latest_channels},
    port::{Port, Ports},
    registry,
    spawn::Spawners,
    spawn_setup::{SpawnSetup, StartupPaths},
};

pub(crate) const INBOX_SLOTS: usize = 256;

#[derive(Debug)]
pub(crate) struct Wiring {
    pub(crate) inbox_receiver: Receiver<Message>,
    pub(crate) inbox: Sender<Message>,
    pub(crate) ports: Ports,
    pub(crate) spectrum_tap: SpectrumTap,
    pub(crate) latest_receivers: LatestReceivers,
    pub(crate) doorbell: Receiver<()>,
    spawners: Spawners,
    paths: StartupPaths,
    latest_senders: LatestSenders,
    #[cfg(target_os = "macos")]
    pub(crate) macos_channel: MacosChannel,
}

impl Wiring {
    pub(crate) fn spawn(
        model: &Model,
        paths: &StartupPaths,
        spawners: &Spawners,
    ) -> Result<Self, SpawnError> {
        let (inbox, inbox_receiver) = bounded(INBOX_SLOTS);
        let (latest_senders, latest_receivers, doorbell) = latest_channels();
        let mut wiring = Self {
            inbox_receiver,
            inbox,
            ports: Ports {
                audio: Port::Closed,
                library: Port::Closed,
                config: Port::Closed,
                macos: Port::Closed,
            },
            spectrum_tap: SpectrumTap::silent(),
            latest_receivers,
            doorbell,
            spawners: *spawners,
            paths: paths.clone(),
            latest_senders,
            #[cfg(target_os = "macos")]
            macos_channel: MacosChannel::new(),
        };
        for row in registry::REGISTRY
            .iter()
            .filter(|row| row.platform.is_present())
        {
            wiring.respawn(row.driver_name, &model.settings.audio_settings)?;
        }
        Ok(wiring)
    }

    pub(crate) fn restart(
        &mut self,
        driver_name: DriverName,
        model: &Model,
    ) -> Option<Message> {
        self.ports.hang_up(driver_name);
        drop(self.ports.join(driver_name));
        match self.respawn(driver_name, &model.settings.audio_settings) {
            Ok(()) => None,
            Err(spawn) => Some(Message::Driver {
                driver_name,
                event: DriverEvent::Died(DriverError::from(&spawn)),
            }),
        }
    }

    fn respawn(
        &mut self,
        driver_name: DriverName,
        audio_settings: &AudioSettings,
    ) -> Result<(), SpawnError> {
        let setup = SpawnSetup {
            audio_settings,
            paths: &self.paths,
            inbox: &self.inbox,
            latest_senders: &self.latest_senders,
            #[cfg(target_os = "macos")]
            macos_channel: &self.macos_channel,
        };
        match driver_name {
            DriverName::Audio => {
                let (thread, spectrum) = (self.spawners.audio)(&setup)?;
                self.spectrum_tap = spectrum;
                self.ports.audio = Port::spawned(driver_name, thread);
            }
            DriverName::Library => {
                let thread = (self.spawners.library)(&setup)?;
                self.ports.library = Port::spawned(driver_name, thread);
            }
            DriverName::Config => {
                let thread = (self.spawners.config)(&setup)?;
                self.ports.config = Port::spawned(driver_name, thread);
            }
            #[cfg(target_os = "macos")]
            DriverName::Macos => {
                let thread = (self.spawners.macos)(&setup)?;
                self.ports.macos = Port::spawned(driver_name, thread);
            }
            #[cfg(not(target_os = "macos"))]
            DriverName::Macos => {}
        }
        Ok(())
    }
}

pub(crate) fn await_exits(
    model: &Model,
    inbox_receiver: &Receiver<Message>,
    timeout: Duration,
) -> Vec<DriverName> {
    let mut awaited_driver_names: Vec<DriverName> = registry::REGISTRY
        .iter()
        .map(|row| row.driver_name)
        .filter(|driver| matches!(model.drivers.status(*driver), DriverStatus::Running))
        .collect();
    let mut reported_driver_names: Vec<DriverName> = registry::REGISTRY
        .iter()
        .map(|row| row.driver_name)
        .filter(|driver| !awaited_driver_names.contains(driver))
        .collect();
    let deadline = Instant::now() + timeout;
    while !awaited_driver_names.is_empty() {
        let Ok(message) = inbox_receiver.recv_deadline(deadline) else {
            break;
        };
        if let Message::Driver {
            driver_name,
            event: DriverEvent::Stopped | DriverEvent::Died(_),
        } = message
        {
            awaited_driver_names.retain(|waiting| *waiting != driver_name);
            reported_driver_names.push(driver_name);
        }
    }
    reported_driver_names
}

pub(crate) fn join_exited(ports: &mut Ports, reported_driver_names: &[DriverName]) {
    for row in registry::REGISTRY {
        if reported_driver_names.contains(&row.driver_name) {
            drop(ports.join(row.driver_name));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use audio::tap::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender};
    use kernel::{
        cmd::{AudioCmd, ConfigCmd, LibraryCmd, MacosCmd},
        domain::driver::DriverName,
        message::{DriverEvent, Message},
    };
    use library::dirs::LibraryDirs;

    use crate::{
        driver_thread::{Congestion, DriverThread, SendError, send},
        latest::{LatestSenders, latest_channels},
        port::{Port, Ports},
        registry,
        spawn::tests::{idle_spawners, spawn_idle},
        spawn_setup::StartupPaths,
        wiring::Wiring,
    };

    fn idle_thread<C: Send + 'static>(
        driver_name: DriverName,
        inbox: &Sender<Message>,
    ) -> DriverThread<C> {
        spawn_idle(registry::row(driver_name), inbox).unwrap()
    }

    pub(crate) fn stub_paths() -> StartupPaths {
        StartupPaths {
            config_paths: config::driver::paths::ConfigPaths {
                config_path: std::path::PathBuf::new(),
                appearance_path: std::path::PathBuf::new(),
                themes_dir: std::path::PathBuf::new(),
                default_music_dir: None,
                theme_name: None,
                seen_texts: config::driver::paths::SeenTexts::default(),
            },
            library_dirs: LibraryDirs::under(std::path::Path::new("")),
        }
    }

    fn idle_library_thread(
        inbox: &Sender<Message>,
        spectrum_sender: Sender<LibraryCmd>,
    ) -> DriverThread<LibraryCmd> {
        let (cmd_sender, cmd_receiver) = crossbeam_channel::unbounded();
        let inbox = inbox.clone();
        let handle = std::thread::spawn(move || {
            for cmd in &cmd_receiver {
                if spectrum_sender.send(cmd).is_err() {
                    break;
                }
            }
            match send(
                &inbox,
                &Congestion::default(),
                Message::Driver {
                    driver_name: DriverName::Library,
                    event: DriverEvent::Stopped,
                },
            ) {
                Ok(()) | Err(SendError::Closed) => {}
            }
        });
        DriverThread {
            cmd_sender,
            handle,
            congestion: Congestion::default(),
        }
    }

    impl Wiring {
        pub(crate) fn idle() -> (Self, Receiver<LibraryCmd>, LatestSenders) {
            let (inbox, inbox_receiver) = crossbeam_channel::unbounded();
            let (library_tap, library_cmd_receiver) = crossbeam_channel::unbounded();
            let (latest_senders, latest_receivers, doorbell) = latest_channels();

            let ports = Ports {
                audio: Port::spawned(
                    DriverName::Audio,
                    idle_thread::<AudioCmd>(DriverName::Audio, &inbox),
                ),
                library: Port::spawned(
                    DriverName::Library,
                    idle_library_thread(&inbox, library_tap),
                ),
                config: Port::spawned(
                    DriverName::Config,
                    idle_thread::<ConfigCmd>(DriverName::Config, &inbox),
                ),
                macos: Port::spawned(
                    DriverName::Macos,
                    idle_thread::<MacosCmd>(DriverName::Macos, &inbox),
                ),
            };

            let paths = stub_paths();

            let wiring = Self {
                inbox_receiver,
                inbox,
                ports,
                spectrum_tap: SpectrumTap::silent(),
                latest_receivers,
                doorbell,
                spawners: idle_spawners(),
                paths,
                latest_senders: latest_senders.clone(),
                #[cfg(target_os = "macos")]
                macos_channel: crate::spawn_setup::MacosChannel::new(),
            };
            (wiring, library_cmd_receiver, latest_senders)
        }
    }
}
