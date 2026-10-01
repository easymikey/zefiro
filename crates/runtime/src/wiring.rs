use std::time::{Duration, Instant};

use audio::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    DriverMessage,
    Message,
    domain::{Driver, DriverStatus, Model},
};

use crate::{
    error::Error,
    latest::{LatestReceivers, LatestSenders, latest_channels},
    port::{LibraryPort, Port, Ports},
    registry,
    runtime::StartupPaths,
    spawn::{SpawnParts, Spawners},
    trace::{Trace, TraceEntry},
};

#[derive(Debug)]
pub(crate) struct Wiring {
    pub(crate) receiver: Receiver<Message>,
    pub(crate) sender: Sender<Message>,
    pub(crate) ports: Ports,
    pub(crate) spectrum: SpectrumTap,
    pub(crate) cells: LatestReceivers,
    pub(crate) notified: Receiver<()>,
    spawners: Spawners,
    paths: StartupPaths,
    writers: LatestSenders,
}

impl Wiring {
    pub(crate) fn spawn(
        model: &Model,
        paths: &StartupPaths,
        spawners: &Spawners,
    ) -> Result<Self, Error> {
        let (sender, arrivals) = bounded(256);
        let (writers, cells, notified) = latest_channels();
        let spawn_parts = SpawnParts {
            model,
            paths,
            sender: &sender,
            writers: &writers,
        };

        let (audio, spectrum) = (spawners.audio)(&spawn_parts)?;
        let library = (spawners.library)(&spawn_parts)?;
        let config = (spawners.config)(&spawn_parts)?;
        let macos = (spawners.macos)(&spawn_parts)?;

        let ports = Ports {
            audio: Port::spawned(Driver::Audio, audio),
            library: LibraryPort::new(Port::spawned(Driver::Library, library)),
            config: Port::spawned(Driver::Config, config),
            macos: Port::spawned(Driver::Macos, macos),
        };

        Ok(Self {
            receiver: arrivals,
            sender,
            ports,
            spectrum,
            cells,
            notified,
            spawners: *spawners,
            paths: paths.clone(),
            writers,
        })
    }

    pub(crate) fn restart(&mut self, driver: Driver, model: &Model) -> Vec<TraceEntry> {
        let join_failed = matches!(self.ports.join(driver), Some(Err(_)))
            .then_some(TraceEntry::JoinFailed { driver });
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        let writers = self.writers.clone();
        let spawn_parts = SpawnParts {
            model,
            paths: &paths,
            sender: &sender,
            writers: &writers,
        };
        let restart_failed = self
            .restart_driver(driver, &spawn_parts)
            .is_err()
            .then_some(TraceEntry::RestartFailed { driver });
        join_failed.into_iter().chain(restart_failed).collect()
    }

    fn restart_driver(
        &mut self,
        driver: Driver,
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<(), Error> {
        match driver {
            Driver::Audio => {
                let (thread, spectrum) = (self.spawners.audio)(spawn_parts)?;
                self.spectrum = spectrum;
                self.ports.audio = Port::spawned(driver, thread);
            }
            Driver::Library => {
                let thread = (self.spawners.library)(spawn_parts)?;
                self.ports.library = LibraryPort::new(Port::spawned(driver, thread));
            }
            Driver::Config => {
                let thread = (self.spawners.config)(spawn_parts)?;
                self.ports.config = Port::spawned(driver, thread);
            }
            Driver::Macos => {
                let thread = (self.spawners.macos)(spawn_parts)?;
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
) -> Vec<Driver> {
    let mut awaited: Vec<Driver> = registry::REGISTRY
        .iter()
        .map(|row| row.driver)
        .filter(|driver| matches!(model.drivers.status(*driver), DriverStatus::Running))
        .collect();
    let mut reported: Vec<Driver> = registry::REGISTRY
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
            event: DriverMessage::Stopped | DriverMessage::Died(_),
        } = message
        {
            awaited.retain(|waiting| *waiting != driver);
            reported.push(driver);
        }
    }
    reported
}

pub(crate) fn join_exited(ports: &mut Ports, reported: &[Driver], trace: &mut Trace) {
    for row in registry::REGISTRY {
        if !reported.contains(&row.driver) {
            continue;
        }
        if matches!(ports.join(row.driver), Some(Err(_) | Ok(Err(_)))) {
            trace.push(TraceEntry::JoinFailed { driver: row.driver });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use audio::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender};
    use kernel::{
        AudioCmd,
        ConfigCmd,
        DriverMessage,
        MacosCmd,
        Message,
        domain::Driver,
    };
    use library::LibraryDirs;

    use crate::{
        config::ConfigPaths,
        driver::DriverThread,
        latest::{LatestSenders, latest_channels},
        library::machine::LibraryMessage,
        port::{LibraryPort, Port, Ports},
        registry,
        runtime::StartupPaths,
        sender::FullEdge,
        spawn::Spawners,
        wiring::Wiring,
    };

    fn idle_thread<C: Send + 'static>(
        driver: Driver,
        inbox: &Sender<Message>,
    ) -> DriverThread<C> {
        crate::driver::spawn_idle(registry::row(driver), inbox).unwrap()
    }

    pub(crate) fn stub_paths() -> StartupPaths {
        StartupPaths {
            config: ConfigPaths {
                config: std::path::PathBuf::new(),
                appearance: std::path::PathBuf::new(),
                themes: std::path::PathBuf::new(),
                theme: None,
                seen: crate::config::SeenTexts::default(),
            },
            library: LibraryDirs::under(std::path::Path::new("")),
        }
    }

    fn idle_library_thread(
        inbox: &Sender<Message>,
        tap: Sender<LibraryMessage>,
    ) -> DriverThread<LibraryMessage> {
        let (commands, command_inbox) = crossbeam_channel::unbounded();
        let report_sender = inbox.clone();
        let handle = std::thread::spawn(move || {
            for command in &command_inbox {
                if tap.send(command).is_err() {
                    break;
                }
            }
            report_sender.send(Message::Driver {
                driver: Driver::Library,
                event: DriverMessage::Stopped,
            })
        });
        DriverThread {
            commands,
            handle,
            full_edge: FullEdge::default(),
        }
    }

    impl Wiring {
        pub(crate) fn idle() -> (Self, Receiver<LibraryMessage>, LatestSenders) {
            let (inbox, arrivals) = crossbeam_channel::unbounded();
            let (library_tap, library_inbox) = crossbeam_channel::unbounded();
            let (writers, cells, notified) = latest_channels();

            let ports = Ports {
                audio: Port::spawned(
                    Driver::Audio,
                    idle_thread::<AudioCmd>(Driver::Audio, &inbox),
                ),
                library: LibraryPort::new(Port::spawned(
                    Driver::Library,
                    idle_library_thread(&inbox, library_tap),
                )),
                config: Port::spawned(
                    Driver::Config,
                    idle_thread::<ConfigCmd>(Driver::Config, &inbox),
                ),
                macos: Port::spawned(
                    Driver::Macos,
                    idle_thread::<MacosCmd>(Driver::Macos, &inbox),
                ),
            };

            let paths = stub_paths();

            let wiring = Self {
                receiver: arrivals,
                sender: inbox,
                ports,
                spectrum: SpectrumTap::silent(),
                cells,
                notified,
                spawners: Spawners::idle(),
                paths,
                writers: writers.clone(),
            };
            (wiring, library_inbox, writers)
        }
    }
}
