use std::time::{Duration, Instant};

use audio::tap::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    domain::{
        driver::{DriverName, DriverStatus},
        model::Model,
    },
    message::{DriverEvent, Message},
};

#[cfg(target_os = "macos")] use crate::spawn_setup::MacosChannel;
use crate::{
    error::Error,
    latest::{LatestReceivers, LatestSenders, latest_channels},
    port::{Port, Ports},
    registry,
    spawn::Spawners,
    spawn_setup::{SpawnSetup, StartupPaths},
};

#[derive(Debug)]
pub(crate) struct Wiring {
    pub(crate) mailbox: Receiver<Message>,
    pub(crate) inbox: Sender<Message>,
    pub(crate) ports: Ports,
    pub(crate) spectrum: SpectrumTap,
    pub(crate) cells: LatestReceivers,
    pub(crate) notified: Receiver<()>,
    spawners: Spawners,
    paths: StartupPaths,
    writers: LatestSenders,
    #[cfg(target_os = "macos")]
    pub(crate) macos: MacosChannel,
}

impl Wiring {
    pub(crate) fn spawn(
        model: &Model,
        paths: &StartupPaths,
        spawners: &Spawners,
    ) -> Result<Self, Error> {
        let (inbox, arrivals) = bounded(256);
        let (writers, cells, notified) = latest_channels();
        #[cfg(target_os = "macos")]
        let macos_channel = MacosChannel::new();
        let setup = SpawnSetup {
            audio: &model.settings.audio,
            paths,
            inbox: &inbox,
            writers: &writers,
            #[cfg(target_os = "macos")]
            macos: &macos_channel,
        };

        let (audio, spectrum) = (spawners.audio)(&setup)?;
        let library = (spawners.library)(&setup)?;
        let config = (spawners.config)(&setup)?;
        #[cfg(target_os = "macos")]
        let port = Port::spawned(DriverName::Macos, (spawners.macos)(&setup)?);
        #[cfg(not(target_os = "macos"))]
        let port = Port::Closed;

        let ports = Ports {
            audio: Port::spawned(DriverName::Audio, audio),
            library: Port::spawned(DriverName::Library, library),
            config: Port::spawned(DriverName::Config, config),
            macos: port,
        };

        Ok(Self {
            mailbox: arrivals,
            inbox,
            ports,
            spectrum,
            cells,
            notified,
            spawners: *spawners,
            paths: paths.clone(),
            writers,
            #[cfg(target_os = "macos")]
            macos: macos_channel,
        })
    }

    pub(crate) fn restart(&mut self, driver: DriverName, model: &Model) {
        self.ports.hang_up(driver);
        drop(self.ports.join(driver));
        let paths = self.paths.clone();
        let inbox = self.inbox.clone();
        let writers = self.writers.clone();
        #[cfg(target_os = "macos")]
        let macos = self.macos.clone();
        let setup = SpawnSetup {
            audio: &model.settings.audio,
            paths: &paths,
            inbox: &inbox,
            writers: &writers,
            #[cfg(target_os = "macos")]
            macos: &macos,
        };
        match self.restart_driver(driver, &setup) {
            Ok(()) | Err(_) => {}
        }
    }

    fn restart_driver(
        &mut self,
        driver: DriverName,
        setup: &SpawnSetup<'_>,
    ) -> Result<(), Error> {
        match driver {
            DriverName::Audio => {
                let (thread, spectrum) = (self.spawners.audio)(setup)?;
                self.spectrum = spectrum;
                self.ports.audio = Port::spawned(driver, thread);
            }
            DriverName::Library => {
                let thread = (self.spawners.library)(setup)?;
                self.ports.library = Port::spawned(driver, thread);
            }
            DriverName::Config => {
                let thread = (self.spawners.config)(setup)?;
                self.ports.config = Port::spawned(driver, thread);
            }
            #[cfg(target_os = "macos")]
            DriverName::Macos => {
                let thread = (self.spawners.macos)(setup)?;
                self.ports.macos = Port::spawned(driver, thread);
            }
            #[cfg(not(target_os = "macos"))]
            DriverName::Macos => {}
        }
        Ok(())
    }
}

pub(crate) fn await_exits(
    model: &Model,
    inbox: &Receiver<Message>,
    timeout: Duration,
) -> Vec<DriverName> {
    let mut awaited: Vec<DriverName> = registry::REGISTRY
        .iter()
        .map(|row| row.driver)
        .filter(|driver| matches!(model.drivers.status(*driver), DriverStatus::Running))
        .collect();
    let mut reported: Vec<DriverName> = registry::REGISTRY
        .iter()
        .map(|row| row.driver)
        .filter(|driver| !awaited.contains(driver))
        .collect();
    let deadline = Instant::now() + timeout;
    while !awaited.is_empty() {
        let Ok(message) = inbox.recv_deadline(deadline) else {
            break;
        };
        if let Message::Driver {
            driver,
            event: DriverEvent::Stopped | DriverEvent::Died(_),
        } = message
        {
            awaited.retain(|waiting| *waiting != driver);
            reported.push(driver);
        }
    }
    reported
}

pub(crate) fn join_exited(ports: &mut Ports, reported: &[DriverName]) {
    for row in registry::REGISTRY {
        if reported.contains(&row.driver) {
            drop(ports.join(row.driver));
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
        driver_thread::{Congestion, DriverThread, send},
        latest::{LatestSenders, latest_channels},
        port::{Port, Ports},
        registry,
        spawn::Spawners,
        spawn_setup::StartupPaths,
        wiring::Wiring,
    };

    fn idle_thread<C: Send + 'static>(
        driver: DriverName,
        inbox: &Sender<Message>,
    ) -> DriverThread<C> {
        crate::driver_thread::spawn_idle(registry::row(driver), inbox).unwrap()
    }

    pub(crate) fn stub_paths() -> StartupPaths {
        StartupPaths {
            config: config::driver::paths::ConfigPaths {
                config: std::path::PathBuf::new(),
                appearance: std::path::PathBuf::new(),
                themes: std::path::PathBuf::new(),
                default_music_dir: None,
                theme: None,
                seen: config::driver::paths::SeenTexts::default(),
            },
            library: LibraryDirs::under(std::path::Path::new("")),
        }
    }

    fn idle_library_thread(
        inbox: &Sender<Message>,
        tap: Sender<LibraryCmd>,
    ) -> DriverThread<LibraryCmd> {
        let (commands, command_inbox) = crossbeam_channel::unbounded();
        let inbox = inbox.clone();
        let handle = std::thread::spawn(move || {
            for command in &command_inbox {
                if tap.send(command).is_err() {
                    break;
                }
            }
            send(
                &inbox,
                &Congestion::default(),
                Message::Driver {
                    driver: DriverName::Library,
                    event: DriverEvent::Stopped,
                },
            )
        });
        DriverThread {
            commands,
            handle,
            full: Congestion::default(),
        }
    }

    impl Wiring {
        pub(crate) fn idle() -> (Self, Receiver<LibraryCmd>, LatestSenders) {
            let (inbox, arrivals) = crossbeam_channel::unbounded();
            let (library_tap, library_inbox) = crossbeam_channel::unbounded();
            let (writers, cells, notified) = latest_channels();

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
                mailbox: arrivals,
                inbox,
                ports,
                spectrum: SpectrumTap::silent(),
                cells,
                notified,
                spawners: Spawners::idle(),
                paths,
                writers: writers.clone(),
                #[cfg(target_os = "macos")]
                macos: crate::spawn_setup::MacosChannel::new(),
            };
            (wiring, library_inbox, writers)
        }
    }
}
