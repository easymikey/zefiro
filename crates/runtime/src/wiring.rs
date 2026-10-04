use std::time::{Duration, Instant};

use audio::tap::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    DriverEvent,
    Message,
    domain::{DriverName, DriverStatus, Model},
};

#[cfg(target_os = "macos")] use crate::macos::MacosChannel;
use crate::{
    error::Error,
    latest::{LatestReceivers, LatestSenders, latest_channels},
    port::{LibraryPort, Port, Ports},
    registry,
    runtime::StartupPaths,
    spawn::{SpawnSetup, Spawners},
    trace::{Trace, TraceEntry, TraceError},
};

#[derive(Debug)]
pub(crate) struct Wiring {
    pub(crate) receiver: Receiver<Message>,
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
            theme: &model.themes.selected,
            paths,
            inbox: &inbox,
            writers: &writers,
            #[cfg(target_os = "macos")]
            macos: &macos_channel,
        };

        let (audio, spectrum) = (spawners.audio)(&setup)?;
        let library = (spawners.library)(&setup)?;
        let config = (spawners.config)(&setup)?;
        let macos = (spawners.macos)(&setup)?;

        let ports = Ports {
            audio: Port::spawned(DriverName::Audio, audio),
            library: LibraryPort::spawned(library),
            config: Port::spawned(DriverName::Config, config),
            macos: Port::spawned(DriverName::Macos, macos),
        };

        Ok(Self {
            receiver: arrivals,
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

    pub(crate) fn restart(
        &mut self,
        driver: DriverName,
        model: &Model,
    ) -> Vec<TraceEntry> {
        let join_failed = matches!(self.ports.join(driver), Some(Err(_)))
            .then_some(TraceEntry::Error(TraceError::Join(driver)));
        let paths = self.paths.clone();
        let inbox = self.inbox.clone();
        let writers = self.writers.clone();
        #[cfg(target_os = "macos")]
        let macos = self.macos.clone();
        let setup = SpawnSetup {
            audio: &model.settings.audio,
            theme: &model.themes.selected,
            paths: &paths,
            inbox: &inbox,
            writers: &writers,
            #[cfg(target_os = "macos")]
            macos: &macos,
        };
        let restart_failed = self
            .restart_driver(driver, &setup)
            .is_err()
            .then_some(TraceEntry::Error(TraceError::Restart(driver)));
        join_failed.into_iter().chain(restart_failed).collect()
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
                let spawned = (self.spawners.library)(setup)?;
                self.ports.library = LibraryPort::spawned(spawned);
            }
            DriverName::Config => {
                let thread = (self.spawners.config)(setup)?;
                self.ports.config = Port::spawned(driver, thread);
            }
            DriverName::Macos => {
                let thread = (self.spawners.macos)(setup)?;
                self.ports.macos = Port::spawned(driver, thread);
            }
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

pub(crate) fn join_exited(
    ports: &mut Ports,
    reported: &[DriverName],
    trace: &mut Trace,
) {
    for row in registry::REGISTRY {
        if !reported.contains(&row.driver) {
            continue;
        }
        if matches!(ports.join(row.driver), Some(Err(_) | Ok(Err(_)))) {
            trace.push(TraceEntry::Error(TraceError::Join(row.driver)));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Instant;

    use audio::tap::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender};
    use kernel::{
        AudioCmd,
        Cmds,
        ConfigCmd,
        DriverEvent,
        LibraryCmd,
        MacosCmd,
        Message,
        domain::DriverName,
    };
    use library::{LibraryDirs, LibraryMessage};

    use crate::{
        driver::DriverThread,
        latest::{LatestSenders, latest_channels},
        outbox::Congestion,
        port::{LibraryPort, Port, Ports},
        registry,
        runtime::StartupPaths,
        spawn::Spawners,
        wiring::Wiring,
    };

    fn idle_thread<C: Send + 'static>(
        driver: DriverName,
        inbox: &Sender<Message>,
    ) -> DriverThread<C> {
        crate::driver::spawn_idle(registry::row(driver), inbox).unwrap()
    }

    pub(crate) fn stub_paths() -> StartupPaths {
        StartupPaths {
            config: config::ConfigPaths {
                config: std::path::PathBuf::new(),
                appearance: std::path::PathBuf::new(),
                themes: std::path::PathBuf::new(),
                theme: None,
                seen: config::SeenTexts::default(),
            },
            library: LibraryDirs::under(std::path::Path::new("")),
        }
    }

    fn idle_library_thread(
        inbox: &Sender<Message>,
        tap: Sender<LibraryMessage>,
    ) -> (DriverThread<LibraryCmd>, Sender<LibraryMessage>) {
        let (commands, command_inbox) = crossbeam_channel::unbounded();
        let inbox = inbox.clone();
        let covers = tap.clone();
        let handle = std::thread::spawn(move || {
            for command in &command_inbox {
                let cmds = Cmds {
                    cmds: vec![command],
                    at: Instant::now(),
                };
                if tap.send(LibraryMessage::Cmds(cmds)).is_err() {
                    break;
                }
            }
            inbox.send(Message::Driver {
                driver: DriverName::Library,
                event: DriverEvent::Stopped,
            })
        });
        let thread = DriverThread {
            commands,
            handle,
            full: Congestion::default(),
        };
        (thread, covers)
    }

    impl Wiring {
        pub(crate) fn idle() -> (Self, Receiver<LibraryMessage>, LatestSenders) {
            let (inbox, arrivals) = crossbeam_channel::unbounded();
            let (library_tap, library_inbox) = crossbeam_channel::unbounded();
            let (writers, cells, notified) = latest_channels();

            let ports = Ports {
                audio: Port::spawned(
                    DriverName::Audio,
                    idle_thread::<AudioCmd>(DriverName::Audio, &inbox),
                ),
                library: LibraryPort::spawned(idle_library_thread(&inbox, library_tap)),
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
                receiver: arrivals,
                inbox,
                ports,
                spectrum: SpectrumTap::silent(),
                cells,
                notified,
                spawners: Spawners::idle(),
                paths,
                writers: writers.clone(),
                #[cfg(target_os = "macos")]
                macos: crate::macos::MacosChannel::new(),
            };
            (wiring, library_inbox, writers)
        }
    }
}
