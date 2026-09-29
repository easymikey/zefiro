use std::{
    thread::JoinHandle,
    time::{Duration, Instant},
};

use audio::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    AudioCmd,
    DriverMessage,
    Message,
    SystemCmd,
    domain::{Driver, DriverStatus, Model},
};

use crate::{
    cells::{Cells, Writers, cells},
    driver::{DriverThread, Report},
    error::RuntimeError,
    interpret::{ConfigCommand, LibraryCommand},
    launch::{Launchers, Launching},
    port::{LibraryPort, Port, Ports},
    registry,
    runtime::BootPaths,
    trace::{Trace, TraceEntry},
};

#[derive(Debug)]
pub(crate) struct Wiring {
    pub(crate) mailbox: Receiver<Message>,
    pub(crate) mailbox_sender: Sender<Message>,
    pub(crate) ports: Ports,
    pub(crate) handles: [Option<JoinHandle<Report>>; 4],
    pub(crate) spectrum: SpectrumTap,
    pub(crate) cells: Cells,
    pub(crate) doorbell: Receiver<()>,
    launchers: Launchers,
    paths: BootPaths,
    writers: Writers,
}

pub(crate) struct Relaunching<'a> {
    pub(crate) model: &'a Model,
    pub(crate) trace: &'a mut Trace,
}

impl Wiring {
    pub(crate) fn spawn(
        model: &Model,
        paths: &BootPaths,
        launchers: &Launchers,
    ) -> Result<Self, RuntimeError> {
        let (mailbox, arrivals) = bounded(256);
        let mailbox_sender = mailbox.clone();
        let (writers, cells, doorbell) = cells();
        let launching = Launching {
            model,
            paths,
            mailbox: &mailbox,
            writers: &writers,
        };

        let audio = (launchers.audio)(&launching)?;
        let library = (launchers.library)(&launching)?;
        let config = (launchers.config)(&launching)?;
        let macos = (launchers.macos)(&launching)?;

        let spectrum = audio.spectrum.ok_or(RuntimeError::NoSpectrum)?;

        let (ports, handles) = wire_ports(Spawned {
            audio: audio.thread,
            library: library.thread,
            config: config.thread,
            macos: macos.thread,
        });

        Ok(Self {
            mailbox: arrivals,
            mailbox_sender,
            ports,
            handles,
            spectrum,
            cells,
            doorbell,
            launchers: *launchers,
            paths: paths.clone(),
            writers,
        })
    }

    pub(crate) fn relaunch(&mut self, driver: Driver, relaunching: Relaunching<'_>) {
        let Relaunching { model, trace } = relaunching;
        if let Some(handle) = take_handle(&mut self.handles, driver)
            && handle.join().is_err()
        {
            trace.push(TraceEntry::JoinFailed { driver });
        }
        let paths = self.paths.clone();
        let mailbox = self.mailbox_sender.clone();
        let writers = self.writers.clone();
        let launching = Launching {
            model,
            paths: &paths,
            mailbox: &mailbox,
            writers: &writers,
        };
        if self.relaunch_driver(driver, &launching).is_err() {
            trace.push(TraceEntry::RestartFailed { driver });
        }
    }

    fn relaunch_driver(
        &mut self,
        driver: Driver,
        launching: &Launching<'_>,
    ) -> Result<(), RuntimeError> {
        match driver {
            Driver::Audio => {
                let launched = (self.launchers.audio)(launching)?;
                self.spectrum = launched.spectrum.ok_or(RuntimeError::NoSpectrum)?;
                self.ports.audio = Port::new(
                    driver,
                    launched.thread.commands,
                    launched.thread.congestion,
                );
                set_handle(&mut self.handles, driver, launched.thread.handle);
            }
            Driver::Library => {
                let launched = (self.launchers.library)(launching)?;
                self.ports.library = LibraryPort::new(Port::new(
                    driver,
                    launched.thread.commands,
                    launched.thread.congestion,
                ));
                set_handle(&mut self.handles, driver, launched.thread.handle);
            }
            Driver::Config => {
                let launched = (self.launchers.config)(launching)?;
                self.ports.config = Port::new(
                    driver,
                    launched.thread.commands,
                    launched.thread.congestion,
                );
                set_handle(&mut self.handles, driver, launched.thread.handle);
            }
            Driver::Macos => {
                let launched = (self.launchers.macos)(launching)?;
                self.ports.macos = Port::new(
                    driver,
                    launched.thread.commands,
                    launched.thread.congestion,
                );
                set_handle(&mut self.handles, driver, launched.thread.handle);
            }
        }
        Ok(())
    }
}

struct Spawned {
    audio: DriverThread<AudioCmd>,
    library: DriverThread<LibraryCommand>,
    config: DriverThread<ConfigCommand>,
    macos: DriverThread<SystemCmd>,
}

fn wire_ports(spawned: Spawned) -> (Ports, [Option<JoinHandle<Report>>; 4]) {
    let Spawned {
        audio,
        library,
        config,
        macos,
    } = spawned;
    let DriverThread {
        commands: audio_commands,
        handle: audio_handle,
        congestion: audio_congestion,
    } = audio;
    let DriverThread {
        commands: library_commands,
        handle: library_handle,
        congestion: library_congestion,
    } = library;
    let DriverThread {
        commands: config_commands,
        handle: config_handle,
        congestion: config_congestion,
    } = config;
    let DriverThread {
        commands: macos_commands,
        handle: macos_handle,
        congestion: macos_congestion,
    } = macos;
    let handles: [Option<JoinHandle<Report>>; 4] = [
        Some(audio_handle),
        Some(library_handle),
        Some(config_handle),
        Some(macos_handle),
    ];
    let ports = Ports {
        audio: Port::new(Driver::Audio, audio_commands, audio_congestion),
        library: LibraryPort::new(Port::new(
            Driver::Library,
            library_commands,
            library_congestion,
        )),
        config: Port::new(Driver::Config, config_commands, config_congestion),
        macos: Port::new(Driver::Macos, macos_commands, macos_congestion),
    };
    (ports, handles)
}

fn take_handle(
    handles: &mut [Option<JoinHandle<Report>>; 4],
    driver: Driver,
) -> Option<JoinHandle<Report>> {
    let [audio, library, config, macos] = handles;
    match driver {
        Driver::Audio => audio.take(),
        Driver::Library => library.take(),
        Driver::Config => config.take(),
        Driver::Macos => macos.take(),
    }
}

fn set_handle(
    handles: &mut [Option<JoinHandle<Report>>; 4],
    driver: Driver,
    handle: JoinHandle<Report>,
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

pub(crate) fn drain_reports(
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

pub(crate) fn join_reported(
    handles: &mut [Option<JoinHandle<Report>>; 4],
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
    use kernel::{AudioCmd, DriverMessage, Message, SystemCmd, domain::Driver};
    use library::LibraryPaths;

    use crate::{
        cells::{Writers, cells},
        config::ConfigPaths,
        driver::{DriverThread, Report},
        interpret::{ConfigCommand, LibraryCommand},
        launch::Launchers,
        mailbox::Congestion,
        port::{LibraryPort, Port, Ports},
        registry,
        runtime::BootPaths,
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

    pub(crate) fn stub_paths() -> BootPaths {
        BootPaths {
            config: ConfigPaths {
                config: None,
                appearance: std::path::PathBuf::new(),
                themes: std::path::PathBuf::new(),
                theme: None,
                seen: crate::config::SeenTexts::default(),
            },
            library: LibraryPaths {
                cache: std::path::PathBuf::new(),
                data: std::path::PathBuf::new(),
                playlists: std::path::PathBuf::new(),
            },
        }
    }

    fn idle_library_thread(
        mailbox: &Sender<Message>,
        tap: Sender<LibraryCommand>,
    ) -> DriverThread<LibraryCommand> {
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
            congestion: Congestion::default(),
        }
    }

    impl Wiring {
        pub(crate) fn idle() -> (Self, Receiver<LibraryCommand>, Writers) {
            let (mailbox, arrivals) = crossbeam_channel::unbounded();
            let (library_tap, library_inbox) = crossbeam_channel::unbounded();
            let (writers, cells, doorbell) = cells();

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
            } = idle_thread::<ConfigCommand>(Driver::Config, &mailbox);
            let DriverThread {
                commands: macos_commands,
                handle: macos_handle,
                ..
            } = idle_thread::<SystemCmd>(Driver::Macos, &mailbox);

            let handles: [Option<JoinHandle<Report>>; 4] = [
                Some(audio_handle),
                Some(library_handle),
                Some(config_handle),
                Some(macos_handle),
            ];

            let ports = Ports {
                audio: Port::new(Driver::Audio, audio_commands, Congestion::default()),
                library: LibraryPort::new(Port::new(
                    Driver::Library,
                    library_commands,
                    Congestion::default(),
                )),
                config: Port::new(
                    Driver::Config,
                    config_commands,
                    Congestion::default(),
                ),
                macos: Port::new(Driver::Macos, macos_commands, Congestion::default()),
            };

            let paths = stub_paths();

            let wiring = Self {
                mailbox: arrivals,
                mailbox_sender: mailbox,
                ports,
                handles,
                spectrum: SpectrumTap::silent(),
                cells,
                doorbell,
                launchers: Launchers::idle(),
                paths,
                writers: writers.clone(),
            };
            (wiring, library_inbox, writers)
        }
    }
}
