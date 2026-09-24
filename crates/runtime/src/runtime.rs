use std::{
    collections::VecDeque,
    mem,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use audio::{AudioLoop, DECODABLE_EXTENSIONS, SpectrumTap};
use crossbeam_channel::{Receiver, unbounded};
use kernel::{
    AudioCmd,
    Cmd,
    DriverMessage,
    Message,
    SystemCmd,
    domain::{Driver, DriverStatus, Model, Startup},
};
use library::LibraryPaths;

use crate::{
    config::{ConfigPaths, ConfigTiming, driver::spawn as spawn_config},
    driver::{Delivery, DriverLoop, DriverThread, spawn_loop},
    error::RuntimeError,
    interpret::{ConfigCommand, Interpreter, Target, gated, interpret},
    library::{
        cover::CoverRequest,
        driver::{LibraryThread, spawn as spawn_library},
    },
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
    pub(crate) audio: DriverThread<AudioCmd>,
    pub(crate) spectrum: SpectrumTap,
    pub(crate) library: LibraryThread,
    pub(crate) config: DriverThread<ConfigCommand>,
    pub(crate) reloads: Receiver<Reload>,
    #[cfg(target_os = "macos")]
    pub(crate) macos: DriverThread<SystemCmd>,
    #[cfg(target_os = "macos")]
    pub(crate) controls: Option<crate::macos::Controls>,
}

impl Wiring {
    fn spawn<A, M>(
        startup: &Startup,
        paths: BootPaths,
        hardware: Hardware<A, M>,
    ) -> Result<Self, RuntimeError>
    where
        A: DriverLoop<AudioCmd>,
        M: DriverLoop<SystemCmd>,
    {
        let (mailbox, arrivals) = unbounded();
        let Hardware {
            audio: audio_loop,
            spectrum,
            system,
        } = hardware;
        let audio = spawn_loop(Driver::Audio, audio_loop, mailbox.clone())?;
        let config_paths = ConfigPaths {
            theme: Some(startup.theme.clone()),
            ..paths.config
        };
        let (config, reloads) =
            spawn_config(config_paths, ConfigTiming::default(), mailbox.clone())?;
        #[cfg(target_os = "macos")]
        let (controls, macos) = crate::macos::spawn(system, mailbox.clone())?;
        #[cfg(not(target_os = "macos"))]
        drop(system);
        let library = spawn_library(paths.library, DECODABLE_EXTENSIONS, mailbox)?;
        Ok(Self {
            mailbox: arrivals,
            audio,
            spectrum,
            library,
            config,
            reloads,
            #[cfg(target_os = "macos")]
            macos,
            #[cfg(target_os = "macos")]
            controls,
        })
    }
}

#[derive(Debug)]
pub struct Runtime {
    pub(crate) model: Model,
    pub(crate) wiring: Wiring,
    pub(crate) timers: Timers,
    pub(crate) trace: Trace,
    flow: Flow,
    shell_effects: Vec<ShellEffect>,
    cover: Option<CoverRequest>,
}

impl Runtime {
    pub(crate) const DRAIN: Duration = Duration::from_secs(2);
    #[cfg(target_os = "macos")]
    const PUMP_CAP: Duration = Duration::from_millis(100);

    pub fn boot<A, M>(
        startup: Startup,
        paths: BootPaths,
        hardware: Hardware<A, M>,
    ) -> Result<Self, RuntimeError>
    where
        A: DriverLoop<AudioCmd>,
        M: DriverLoop<SystemCmd>,
    {
        let wiring = Wiring::spawn(&startup, paths, hardware)?;
        let mut trace = Trace::default();
        #[cfg(target_os = "macos")]
        if wiring.controls.is_none() {
            trace.push(TraceEntry::ControlsUnattached);
        }
        Ok(Self::assemble(startup, wiring, trace))
    }

    pub(crate) fn assemble(startup: Startup, wiring: Wiring, trace: Trace) -> Self {
        let (model, cmd) = kernel::startup(startup);
        let mut runtime = Self {
            model,
            wiring,
            timers: Timers::default(),
            trace,
            flow: Flow::Continue,
            shell_effects: Vec::new(),
            cover: None,
        };
        for answer in runtime.interpret(cmd) {
            runtime.step(answer);
        }
        #[cfg(not(target_os = "macos"))]
        runtime.step(Message::Driver(Driver::Macos, DriverMessage::Stopped));
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
        }
    }

    pub(crate) fn request_cover(&mut self, request: CoverRequest) {
        if self.cover.as_ref() == Some(&request) {
            return;
        }
        let target = Target {
            driver: Driver::Library,
            sender: &self.wiring.library.covers,
        };
        let sent = gated(&self.model.drivers, target, request.clone());
        self.cover = Some(request);
        if sent.is_err() {
            self.trace.push(TraceEntry::Dropped {
                driver: Driver::Library,
                command: "cover",
            });
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn pump(&self) -> Option<Instant> {
        let marker = self
            .wiring
            .controls
            .as_ref()
            .and(crate::macos::MainThreadMarker::new())?;
        crate::macos::pump(marker);
        Some(Instant::now() + Self::PUMP_CAP)
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) const fn pump(&self) -> Option<Instant> {
        None
    }

    pub(crate) fn drain(self) {
        let Self {
            model,
            wiring,
            mut trace,
            ..
        } = self;
        #[cfg(target_os = "macos")]
        drop(wiring.controls);
        let LibraryThread { thread, covers, .. } = wiring.library;
        drop(covers);
        let mut handles = vec![
            release(Driver::Audio, wiring.audio),
            release(Driver::Library, thread),
            release(Driver::Config, wiring.config),
        ];
        #[cfg(target_os = "macos")]
        handles.push(release(Driver::Macos, wiring.macos));
        let mut awaited: Vec<Driver> = handles
            .iter()
            .map(|(driver, _)| *driver)
            .filter(|driver| {
                matches!(model.drivers.status(*driver), DriverStatus::Running)
            })
            .collect();
        let mut reported: Vec<Driver> = handles
            .iter()
            .map(|(driver, _)| *driver)
            .filter(|driver| !awaited.contains(driver))
            .collect();
        let deadline = Instant::now() + Self::DRAIN;
        while !awaited.is_empty() {
            let Ok(message) = wiring.mailbox.recv_deadline(deadline) else {
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
        for (driver, handle) in handles {
            if !reported.contains(&driver) {
                continue;
            }
            let joined = handle.join();
            if matches!(joined, Err(_) | Ok(Err(_))) {
                trace.push(TraceEntry::JoinFailed { driver });
            }
        }
    }

    fn update(&mut self, message: Message) -> (Change, Vec<Message>) {
        let label: &'static str = (&message).into();
        match kernel::update::update(&mut self.model, message) {
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
            audio: &self.wiring.audio.commands,
            library: &self.wiring.library.thread.commands,
            config: &self.wiring.config.commands,
            #[cfg(target_os = "macos")]
            macos: Some(&self.wiring.macos.commands),
            #[cfg(not(target_os = "macos"))]
            macos: None,
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

fn release<C>(
    driver: Driver,
    thread: DriverThread<C>,
) -> (Driver, JoinHandle<Delivery>) {
    let DriverThread { commands, handle } = thread;
    drop(commands);
    (driver, handle)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use audio::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        DriverMessage,
        Message,
        Toast,
        WorkspaceRequest,
        domain::{Driver, DriverStatus, Startup},
    };
    use rstest::rstest;

    use crate::{
        driver::{DriverThread, NoDriver, spawn_loop},
        library::{cover::CoverRequest, driver::LibraryThread},
        runtime::{Change, Runtime, Wiring},
        trace::{Trace, TraceEntry},
    };

    fn idle<C: Send + 'static>(
        driver: Driver,
        mailbox: &Sender<Message>,
    ) -> DriverThread<C> {
        spawn_loop(driver, NoDriver, mailbox.clone()).unwrap()
    }

    fn stock_startup() -> Startup {
        Startup::default()
    }

    fn boot(library: DriverStatus) -> (Runtime, Receiver<CoverRequest>) {
        let (mailbox, arrivals) = unbounded();
        let (covers, cover_inbox) = unbounded();
        let (_reloads_sender, reloads) = unbounded();
        let (_decoded_sender, decoded) = unbounded();
        let wiring = Wiring {
            mailbox: arrivals,
            audio: idle(Driver::Audio, &mailbox),
            spectrum: SpectrumTap::silent(),
            library: LibraryThread {
                thread: idle(Driver::Library, &mailbox),
                covers,
                decoded,
            },
            config: idle(Driver::Config, &mailbox),
            reloads,
            #[cfg(target_os = "macos")]
            macos: idle(Driver::Macos, &mailbox),
            #[cfg(target_os = "macos")]
            controls: None,
        };
        let mut runtime = Runtime::assemble(stock_startup(), wiring, Trace::default());
        runtime.model.drivers.library = library;
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
}
