use std::{
    thread::JoinHandle,
    time::{Duration, Instant},
};

use audio::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    AudioCmd,
    ConfigCmd,
    DriverMessage,
    MacosCmd,
    Message,
    domain::{Driver, DriverStatus, Model},
};

use crate::{
    cells::{Receivers, Senders, cells},
    driver::{DriverThread, Exit},
    error::Error,
    library::machine::LibraryMessage,
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
    pub(crate) handles: [Option<JoinHandle<Exit>>; 4],
    pub(crate) spectrum: SpectrumTap,
    pub(crate) cells: Receivers,
    pub(crate) notified: Receiver<()>,
    spawners: Spawners,
    paths: StartupPaths,
    writers: Senders,
}

pub(crate) struct RestartParts<'a> {
    pub(crate) model: &'a Model,
    pub(crate) trace: &'a mut Trace,
}

impl Wiring {
    pub(crate) fn spawn(
        model: &Model,
        paths: &StartupPaths,
        spawners: &Spawners,
    ) -> Result<Self, Error> {
        let (sender, arrivals) = bounded(256);
        let (writers, cells, notified) = cells();
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

        let (ports, handles) = split_spawned(Spawned {
            audio,
            library,
            config,
            macos,
        });

        Ok(Self {
            receiver: arrivals,
            sender,
            ports,
            handles,
            spectrum,
            cells,
            notified,
            spawners: *spawners,
            paths: paths.clone(),
            writers,
        })
    }

    pub(crate) fn restart(&mut self, driver: Driver, restart_parts: RestartParts<'_>) {
        let RestartParts { model, trace } = restart_parts;
        if let Some(handle) = take_handle(&mut self.handles, driver)
            && handle.join().is_err()
        {
            trace.push(TraceEntry::JoinFailed { driver });
        }
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        let writers = self.writers.clone();
        let spawn_parts = SpawnParts {
            model,
            paths: &paths,
            sender: &sender,
            writers: &writers,
        };
        if self.restart_driver(driver, &spawn_parts).is_err() {
            trace.push(TraceEntry::RestartFailed { driver });
        }
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
                self.ports.audio = Port::new(driver, thread.commands, thread.full_edge);
                set_handle(&mut self.handles, driver, thread.handle);
            }
            Driver::Library => {
                let thread = (self.spawners.library)(spawn_parts)?;
                self.ports.library = LibraryPort::new(Port::new(
                    driver,
                    thread.commands,
                    thread.full_edge,
                ));
                set_handle(&mut self.handles, driver, thread.handle);
            }
            Driver::Config => {
                let thread = (self.spawners.config)(spawn_parts)?;
                self.ports.config =
                    Port::new(driver, thread.commands, thread.full_edge);
                set_handle(&mut self.handles, driver, thread.handle);
            }
            Driver::Macos => {
                let thread = (self.spawners.macos)(spawn_parts)?;
                self.ports.macos = Port::new(driver, thread.commands, thread.full_edge);
                set_handle(&mut self.handles, driver, thread.handle);
            }
        }
        Ok(())
    }
}

struct Spawned {
    audio: DriverThread<AudioCmd>,
    library: DriverThread<LibraryMessage>,
    config: DriverThread<ConfigCmd>,
    macos: DriverThread<MacosCmd>,
}

fn split_spawned(spawned: Spawned) -> (Ports, [Option<JoinHandle<Exit>>; 4]) {
    let Spawned {
        audio,
        library,
        config,
        macos,
    } = spawned;
    let DriverThread {
        commands: audio_commands,
        handle: audio_handle,
        full_edge: audio_full_edge,
    } = audio;
    let DriverThread {
        commands: library_commands,
        handle: library_handle,
        full_edge: library_full_edge,
    } = library;
    let DriverThread {
        commands: config_commands,
        handle: config_handle,
        full_edge: config_full_edge,
    } = config;
    let DriverThread {
        commands: macos_commands,
        handle: macos_handle,
        full_edge: macos_full_edge,
    } = macos;
    let handles: [Option<JoinHandle<Exit>>; 4] = [
        Some(audio_handle),
        Some(library_handle),
        Some(config_handle),
        Some(macos_handle),
    ];
    let ports = Ports {
        audio: Port::new(Driver::Audio, audio_commands, audio_full_edge),
        library: LibraryPort::new(Port::new(
            Driver::Library,
            library_commands,
            library_full_edge,
        )),
        config: Port::new(Driver::Config, config_commands, config_full_edge),
        macos: Port::new(Driver::Macos, macos_commands, macos_full_edge),
    };
    (ports, handles)
}

fn take_handle(
    handles: &mut [Option<JoinHandle<Exit>>; 4],
    driver: Driver,
) -> Option<JoinHandle<Exit>> {
    let [audio, library, config, macos] = handles;
    match driver {
        Driver::Audio => audio.take(),
        Driver::Library => library.take(),
        Driver::Config => config.take(),
        Driver::Macos => macos.take(),
    }
}

fn set_handle(
    handles: &mut [Option<JoinHandle<Exit>>; 4],
    driver: Driver,
    handle: JoinHandle<Exit>,
) {
    let [audio, library, config, macos] = handles;
    match driver {
        Driver::Audio => *audio = Some(handle),
        Driver::Library => *library = Some(handle),
        Driver::Config => *config = Some(handle),
        Driver::Macos => *macos = Some(handle),
    }
}

pub(crate) fn drop_ports(ports: Ports) {
    let Ports {
        audio,
        macos,
        library,
        config,
    } = ports;
    drop(audio);
    drop(macos);
    drop(library);
    drop(config);
}

pub(crate) fn await_exits(
    model: &Model,
    mailbox: &Receiver<Message>,
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
        let Ok(message) = mailbox.recv_deadline(deadline) else {
            break;
        };
        if let Message::Driver(
            driver,
            DriverMessage::Stopped | DriverMessage::Died(_),
        ) = message
        {
            awaited.retain(|waiting| *waiting != driver);
            reported.push(driver);
        }
    }
    reported
}

pub(crate) fn join_exited(
    handles: &mut [Option<JoinHandle<Exit>>; 4],
    reported: &[Driver],
    trace: &mut Trace,
) {
    for row in registry::REGISTRY {
        if !reported.contains(&row.driver) {
            continue;
        }
        let Some(handle) = take_handle(handles, row.driver) else {
            continue;
        };
        let joined = handle.join();
        if matches!(joined, Err(_) | Ok(Err(_))) {
            trace.push(TraceEntry::JoinFailed { driver: row.driver });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::thread::JoinHandle;

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
        cells::{Senders, cells},
        config::ConfigPaths,
        driver::{DriverThread, Exit},
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
        mailbox: &Sender<Message>,
    ) -> DriverThread<C> {
        crate::driver::spawn_loop::<C, Message, _>(
            registry::row(driver),
            crate::driver::NoDriver,
            mailbox,
        )
        .unwrap()
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
            library: LibraryDirs {
                cache_dir: std::path::PathBuf::new(),
                data_dir: std::path::PathBuf::new(),
                playlists_dir: std::path::PathBuf::new(),
            },
        }
    }

    fn idle_library_thread(
        mailbox: &Sender<Message>,
        tap: Sender<LibraryMessage>,
    ) -> DriverThread<LibraryMessage> {
        let (commands, inbox) = crossbeam_channel::unbounded();
        let report_sender = mailbox.clone();
        let handle = std::thread::spawn(move || {
            for command in &inbox {
                if tap.send(command).is_err() {
                    break;
                }
            }
            report_sender.send(Message::Driver(Driver::Library, DriverMessage::Stopped))
        });
        DriverThread {
            commands,
            handle,
            full_edge: FullEdge::default(),
        }
    }

    impl Wiring {
        pub(crate) fn idle() -> (Self, Receiver<LibraryMessage>, Senders) {
            let (mailbox, arrivals) = crossbeam_channel::unbounded();
            let (library_tap, library_inbox) = crossbeam_channel::unbounded();
            let (writers, cells, notified) = cells();

            let DriverThread {
                commands: audio_commands,
                handle: audio_handle,
                ..
            } = idle_thread::<AudioCmd>(Driver::Audio, &mailbox);
            let DriverThread {
                commands: library_commands,
                handle: library_handle,
                ..
            } = idle_library_thread(&mailbox, library_tap);
            let DriverThread {
                commands: config_commands,
                handle: config_handle,
                ..
            } = idle_thread::<ConfigCmd>(Driver::Config, &mailbox);
            let DriverThread {
                commands: macos_commands,
                handle: macos_handle,
                ..
            } = idle_thread::<MacosCmd>(Driver::Macos, &mailbox);

            let handles: [Option<JoinHandle<Exit>>; 4] = [
                Some(audio_handle),
                Some(library_handle),
                Some(config_handle),
                Some(macos_handle),
            ];

            let ports = Ports {
                audio: Port::new(Driver::Audio, audio_commands, FullEdge::default()),
                library: LibraryPort::new(Port::new(
                    Driver::Library,
                    library_commands,
                    FullEdge::default(),
                )),
                config: Port::new(Driver::Config, config_commands, FullEdge::default()),
                macos: Port::new(Driver::Macos, macos_commands, FullEdge::default()),
            };

            let paths = stub_paths();

            let wiring = Self {
                receiver: arrivals,
                sender: mailbox,
                ports,
                handles,
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
