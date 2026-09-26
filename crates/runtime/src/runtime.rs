use std::{
    collections::VecDeque,
    mem,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use audio::{AudioLoop, DECODABLE_EXTENSIONS, SpectrumTap};
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{
    AudioCmd,
    AudioEvent,
    Cmd,
    DriverMessage,
    LibraryCmd,
    Message,
    Moment,
    SystemCmd,
    SystemEvent,
    domain::{Driver, DriverStatus, Model, Startup},
};
use library::LibraryPaths;

use crate::{
    config::{ConfigPaths, ConfigTiming, driver::spawn as spawn_config},
    driver::{DriverLoop, DriverThread, Report, spawn_loop},
    error::RuntimeError,
    interpret::{ConfigCommand, Interpreter, interpret},
    library::{
        cover::{CoverDecoded, CoverRequest},
        driver::spawn as spawn_library,
    },
    mailbox::Crowding,
    port::{Port, Ports},
    registry,
    shell::{Flow, Reload, ShellEffect, View},
    timers::Timers,
    trace::{Trace, TraceEntry},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Applied,
    Refused,
}

#[derive(Debug, Clone)]
pub struct BootPaths {
    pub config: ConfigPaths,
    pub library: LibraryPaths,
}

#[derive(Debug)]
pub struct Hardware<A, M> {
    audio: A,
    spectrum: SpectrumTap,
    system: M,
}

impl<A, M> Hardware<A, M> {
    #[must_use]
    pub fn new(audio: A, spectrum: SpectrumTap, system: M) -> Self {
        Self {
            audio,
            spectrum,
            system,
        }
    }
}

#[cfg(target_os = "macos")]
impl Hardware<AudioLoop, crate::macos::SystemStart> {
    #[must_use]
    pub fn system(startup: &Startup) -> Self {
        let (audio, spectrum) = crate::audio::prepare(startup);
        Self {
            audio,
            spectrum,
            system: crate::macos::SystemStart::new(library::embedded_cover),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl Hardware<AudioLoop, crate::driver::NoDriver> {
    #[must_use]
    pub fn system(startup: &Startup) -> Self {
        let (audio, spectrum) = crate::audio::prepare(startup);
        Self {
            audio,
            spectrum,
            system: crate::driver::NoDriver,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Wiring {
    pub(crate) mailbox: Receiver<Message>,
    pub(crate) mailbox_sender: Sender<Message>,
    pub(crate) ports: Ports,
    pub(crate) handles: [Option<JoinHandle<Report>>; 4],
    pub(crate) spectrum: SpectrumTap,
    pub(crate) reloads: Receiver<Reload>,
    pub(crate) decoded: Receiver<CoverDecoded>,
}

#[cfg(target_os = "macos")]
fn spawn_macos<M: DriverLoop<SystemCmd, SystemEvent>>(
    system: M,
    mailbox: &Sender<Message>,
) -> Result<Option<DriverThread<SystemCmd>>, RuntimeError> {
    Ok(Some(crate::macos::spawn(system, mailbox)?))
}

#[cfg(not(target_os = "macos"))]
fn spawn_macos<M: DriverLoop<SystemCmd, SystemEvent>>(
    system: M,
    _mailbox: &Sender<Message>,
) -> Result<Option<DriverThread<SystemCmd>>, RuntimeError> {
    drop(system);
    Ok(None)
}

impl Wiring {
    fn spawn<A, M>(
        startup: &Startup,
        paths: BootPaths,
        hardware: Hardware<A, M>,
    ) -> Result<Self, RuntimeError>
    where
        A: DriverLoop<AudioCmd, AudioEvent>,
        M: DriverLoop<SystemCmd, SystemEvent>,
    {
        for row in registry::REGISTRY {
            debug_assert!(row.inbox > 0);
        }
        let (mailbox, arrivals) = bounded(256);
        let mailbox_sender = mailbox.clone();
        let Hardware {
            audio: audio_loop,
            spectrum,
            system,
        } = hardware;
        let audio = spawn_loop(registry::row(Driver::Audio), audio_loop, &mailbox)?;
        let config_paths = ConfigPaths {
            theme: Some(startup.theme.to_string()),
            ..paths.config
        };
        let (config, reloads) =
            spawn_config(config_paths, ConfigTiming::default(), &mailbox)?;
        let macos = spawn_macos(system, &mailbox)?;
        let (library, covers, decoded) =
            spawn_library(paths.library, DECODABLE_EXTENSIONS, &mailbox)?;
        let (ports, handles) = wire_ports(Spawned {
            audio,
            library,
            config,
            macos,
            covers,
        });

        Ok(Self {
            mailbox: arrivals,
            mailbox_sender,
            ports,
            handles,
            spectrum,
            reloads,
            decoded,
        })
    }
}

struct Spawned {
    audio: DriverThread<AudioCmd>,
    library: DriverThread<LibraryCmd>,
    config: DriverThread<ConfigCommand>,
    macos: Option<DriverThread<SystemCmd>>,
    covers: Sender<CoverRequest>,
}

fn wire_ports(spawned: Spawned) -> (Ports, [Option<JoinHandle<Report>>; 4]) {
    let Spawned {
        audio,
        library,
        config,
        macos,
        covers,
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
    let (macos_port, macos_handle) = match macos {
        Some(DriverThread {
            commands,
            handle,
            congestion,
        }) => {
            freshly_spawned(&congestion);
            (Some(Port::new(Driver::Macos, commands)), Some(handle))
        }
        None => (None, None),
    };
    freshly_spawned(&audio_congestion);
    freshly_spawned(&library_congestion);
    freshly_spawned(&config_congestion);
    let handles: [Option<JoinHandle<Report>>; 4] = [
        Some(audio_handle),
        Some(library_handle),
        Some(config_handle),
        macos_handle,
    ];
    let ports = Ports {
        audio: Port::new(Driver::Audio, audio_commands),
        library: Port::new(Driver::Library, library_commands),
        covers: Port::new(Driver::Library, covers),
        config: Port::new(Driver::Config, config_commands),
        macos: macos_port,
    };
    (ports, handles)
}

fn freshly_spawned(congestion: &crate::mailbox::Congestion) {
    debug_assert!(matches!(congestion.settle(), Crowding::Clear));
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

fn drop_ports(ports: Ports) {
    let Ports {
        audio,
        macos,
        library,
        config,
        covers,
    } = ports;
    drop(audio);
    drop(macos);
    drop(library);
    drop(config);
    drop(covers);
}

fn drain_reports(
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

fn join_reported(
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
fn idle_thread<C: Send + 'static>(
    driver: Driver,
    mailbox: &Sender<Message>,
) -> DriverThread<C> {
    spawn_loop::<C, Message, _>(registry::row(driver), crate::driver::NoDriver, mailbox)
        .unwrap()
}

#[cfg(test)]
impl Wiring {
    pub(crate) fn idle() -> (
        Self,
        Receiver<CoverRequest>,
        Sender<Reload>,
        Sender<CoverDecoded>,
    ) {
        let (mailbox, arrivals) = crossbeam_channel::unbounded();
        let (covers, cover_inbox) = crossbeam_channel::unbounded();
        let (reloads_sender, reloads) = crossbeam_channel::unbounded();
        let (decoded_sender, decoded) = crossbeam_channel::unbounded();

        let DriverThread {
            commands: audio_commands,
            handle: audio_handle,
            ..
        } = idle_thread::<AudioCmd>(Driver::Audio, &mailbox);
        let DriverThread {
            commands: library_commands,
            handle: library_handle,
            ..
        } = idle_thread::<LibraryCmd>(Driver::Library, &mailbox);
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
            audio: Port::new(Driver::Audio, audio_commands),
            library: Port::new(Driver::Library, library_commands),
            covers: Port::new(Driver::Library, covers),
            config: Port::new(Driver::Config, config_commands),
            macos: Some(Port::new(Driver::Macos, macos_commands)),
        };

        let wiring = Self {
            mailbox: arrivals,
            mailbox_sender: mailbox,
            ports,
            handles,
            spectrum: SpectrumTap::silent(),
            reloads,
            decoded,
        };
        (wiring, cover_inbox, reloads_sender, decoded_sender)
    }
}

#[derive(Debug)]
pub struct Runtime {
    pub(crate) model: Model,
    pub(crate) wiring: Wiring,
    pub(crate) timers: Timers,
    pub(crate) trace: Trace,
    epoch: Instant,
    flow: Flow,
    shell_effects: Vec<ShellEffect>,
    cover: Option<CoverRequest>,
}

impl Runtime {
    pub(crate) const DRAIN: Duration = Duration::from_secs(2);

    pub fn boot<A, M>(
        startup: Startup,
        paths: BootPaths,
        hardware: Hardware<A, M>,
    ) -> Result<Self, RuntimeError>
    where
        A: DriverLoop<AudioCmd, AudioEvent>,
        M: DriverLoop<SystemCmd, SystemEvent>,
    {
        let wiring = Wiring::spawn(&startup, paths, hardware)?;
        let trace = Trace::default();
        Ok(Self::assemble(startup, wiring, trace))
    }

    pub(crate) fn assemble(startup: Startup, wiring: Wiring, trace: Trace) -> Self {
        let (mut model, cmd) = kernel::startup(startup);
        for row in registry::REGISTRY {
            model.drivers = model.drivers.with_strategy(row.driver, row.supervision);
        }
        let mut runtime = Self {
            model,
            wiring,
            timers: Timers::default(),
            trace,
            epoch: Instant::now(),
            flow: Flow::Continue,
            shell_effects: Vec::new(),
            cover: None,
        };
        for answer in runtime.interpret(cmd) {
            runtime.step(answer);
        }
        for row in registry::REGISTRY {
            if !row.platform.present() {
                runtime.step(Message::Driver(row.driver, DriverMessage::Stopped));
            }
        }
        runtime
    }

    pub(crate) fn step(&mut self, message: Message) -> Change {
        let mut queue: VecDeque<Message> = VecDeque::from([message]);
        let mut change = Change::Refused;
        while let Some(current) = queue.pop_front() {
            let (applied, following) = self.update(current);
            if let Change::Applied = applied {
                change = Change::Applied;
            }
            queue.extend(following);
        }
        change
    }

    #[must_use]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    #[must_use]
    pub(crate) fn sleep_deadline(&self) -> Option<Instant> {
        self.timers.sleep_deadline(self.model.transport.sleep)
    }

    pub(crate) fn flow(&self) -> Flow {
        self.flow
    }

    pub(crate) fn take_shell_effects(&mut self) -> Vec<ShellEffect> {
        mem::take(&mut self.shell_effects)
    }

    pub(crate) fn view(&self) -> View<'_> {
        View {
            model: &self.model,
            spectrum: &self.wiring.spectrum,
            sleep_deadline: self.sleep_deadline(),
            now: self.now(),
        }
    }

    pub(crate) fn now(&self) -> Moment {
        Moment::new(Instant::now().saturating_duration_since(self.epoch))
    }

    pub(crate) fn request_cover(&mut self, request: CoverRequest) {
        if self.cover.as_ref() == Some(&request) {
            return;
        }
        let sent = self
            .wiring
            .ports
            .covers
            .send(&self.model.drivers, request.clone());
        self.cover = Some(request);
        if let Err(undelivered) = sent {
            self.trace.push(undelivered.into());
        }
    }

    #[must_use]
    pub(crate) fn mailbox_sender(&self) -> Sender<Message> {
        self.wiring.mailbox_sender.clone()
    }

    pub(crate) fn drain(self) {
        let Self {
            model,
            wiring,
            mut trace,
            ..
        } = self;
        let Wiring {
            mailbox,
            mut handles,
            ports,
            ..
        } = wiring;
        drop_ports(ports);
        let reported = drain_reports(&model, &mailbox, Self::DRAIN);
        join_reported(&mut handles, &reported, &mut trace);
    }

    fn update(&mut self, message: Message) -> (Change, Vec<Message>) {
        let label: &'static str = (&message).into();
        let now = self.now();
        match kernel::update::update(&mut self.model, message, now) {
            Ok(cmd) => (Change::Applied, self.interpret(cmd)),
            Err(rejection) => {
                self.trace.push(TraceEntry::Rejected {
                    message: label,
                    rejection,
                });
                (Change::Refused, Vec::new())
            }
        }
    }

    fn interpret(&mut self, cmd: Cmd) -> Vec<Message> {
        let mut interpreter = Interpreter {
            drivers: &self.model.drivers,
            ports: &self.wiring.ports,
            timers: &mut self.timers,
            trace: &mut self.trace,
        };
        let interpreted = interpret(cmd, &mut interpreter);
        self.shell_effects.extend(interpreted.shell);
        if let Flow::Stop = interpreted.flow {
            self.flow = Flow::Stop;
        }
        interpreted.answers
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossbeam_channel::Receiver;
    use kernel::{
        DriverMessage,
        Message,
        Toast,
        WorkspaceRequest,
        domain::{Driver, DriverStatus, Startup},
    };
    use rstest::rstest;

    use crate::{
        library::cover::CoverRequest,
        runtime::{Change, Runtime, Wiring},
        trace::{Trace, TraceEntry},
    };

    fn stock_startup() -> Startup {
        Startup::default()
    }

    fn boot(library: DriverStatus) -> (Runtime, Receiver<CoverRequest>) {
        let (wiring, cover_inbox, _reloads_sender, _decoded_sender) = Wiring::idle();
        let mut runtime = Runtime::assemble(stock_startup(), wiring, Trace::default());
        runtime.model.drivers.record_mut(Driver::Library).status = library;
        (runtime, cover_inbox)
    }

    #[test]
    fn a_refused_cover_request_is_traced_once_not_on_every_paint() {
        let (mut runtime, _cover_inbox) = boot(DriverStatus::Stopped);
        let request = CoverRequest {
            path: PathBuf::from("/music/cover.jpg"),
            side: 64,
        };

        runtime.request_cover(request.clone());
        runtime.request_cover(request.clone());
        runtime.request_cover(request);

        let dropped = runtime
            .trace
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    TraceEntry::Dropped {
                        driver: Driver::Library,
                        command: "cover"
                    }
                )
            })
            .count();
        assert_eq!(dropped, 1);
        runtime.drain();
    }

    #[test]
    fn a_repeated_accepted_cover_request_is_sent_once() {
        let (mut runtime, cover_inbox) = boot(DriverStatus::Running);
        let request = CoverRequest {
            path: PathBuf::from("/music/cover.jpg"),
            side: 64,
        };

        runtime.request_cover(request.clone());
        runtime.request_cover(request);

        assert!(runtime.trace.is_empty());
        assert_eq!(cover_inbox.try_iter().count(), 1);
        runtime.drain();
    }

    #[rstest]
    #[case::an_accepted_message_applies(
        DriverStatus::Running,
        Message::Workspace(WorkspaceRequest::ShowToast(Toast::error("hello".to_owned()))),
        Change::Applied
    )]
    #[case::a_rejected_message_is_refused(
        DriverStatus::Stopped,
        Message::Driver(Driver::Library, DriverMessage::Stopped),
        Change::Refused
    )]
    fn step_reports_whether_the_message_changed_the_model(
        #[case] library: DriverStatus,
        #[case] message: Message,
        #[case] expected: Change,
    ) {
        let (mut runtime, _cover_inbox) = boot(library);

        let change = runtime.step(message);

        assert_eq!(change, expected);
        runtime.drain();
    }

    #[test]
    fn runtime_is_send() {
        fn sendable<T: Send>() {}
        sendable::<Runtime>();
    }
}
