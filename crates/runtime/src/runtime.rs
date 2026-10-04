use std::{
    collections::VecDeque,
    mem,
    ops::ControlFlow,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use kernel::{
    cmd::Effect,
    domain::{model::Model, startup::Startup, time::Moment},
    message::{DriverEvent, Message},
    update::machine::Unhandled,
};

use crate::{
    error::{ClockError, Error},
    interpret::{Interpreter, interpret},
    registry,
    shell::{Frame, ShellEffect},
    spawn::Spawners,
    startup_paths::StartupPaths,
    timers::Timers,
    trace::Trace,
    wiring::{Wiring, await_exits, join_exited},
};

#[derive(Debug)]
pub struct Runtime {
    pub(crate) model: Model,
    pub(crate) wiring: Wiring,
    pub(crate) timers: Timers,
    pub(crate) trace: Trace,
    epoch: Instant,
    unix_offset: Duration,
    flow: ControlFlow<()>,
    shell_effects: Vec<ShellEffect>,
}

impl Runtime {
    pub(crate) const DRAIN: Duration = Duration::from_secs(2);

    pub fn start(
        startup: Startup,
        paths: &StartupPaths,
        spawners: &Spawners,
    ) -> Result<Self, Error> {
        let (model, effects) = kernel::update::startup::startup(startup);
        let wiring = Wiring::spawn(&model, paths, spawners)?;
        Ok(Self::assemble((model, effects), wiring, Trace::default())?)
    }

    pub(crate) fn assemble(
        started: (Model, Vec<Effect>),
        wiring: Wiring,
        trace: Trace,
    ) -> Result<Self, ClockError> {
        let (model, effects) = started;
        let mut runtime = Self {
            model,
            wiring,
            timers: Timers::default(),
            trace,
            epoch: Instant::now(),
            unix_offset: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(ClockError)?,
            flow: ControlFlow::Continue(()),
            shell_effects: Vec::new(),
        };
        for answer in runtime.interpret(effects) {
            match runtime.deliver(answer) {
                Ok(()) | Err(Unhandled) => {}
            }
        }
        for row in registry::REGISTRY {
            if !row.platform.present() {
                match runtime.deliver(Message::Driver {
                    driver: row.driver,
                    event: DriverEvent::Stopped,
                }) {
                    Ok(()) | Err(Unhandled) => {}
                }
            }
        }
        Ok(runtime)
    }

    pub(crate) fn deliver(&mut self, message: Message) -> Result<(), Unhandled> {
        let mut queue: VecDeque<Message> = VecDeque::from([message]);
        let mut result = Err(Unhandled);
        while let Some(current) = queue.pop_front() {
            if let Ok(following) = self.update(current) {
                result = Ok(());
                queue.extend(following);
            }
        }
        result
    }

    pub(crate) fn report_full(&mut self) {
        for row in registry::REGISTRY {
            if self.flow.is_break() {
                return;
            }
            if self.wiring.ports.full(row.driver).take() {
                match self.deliver(Message::Driver {
                    driver: row.driver,
                    event: DriverEvent::Full,
                }) {
                    Ok(()) | Err(Unhandled) => {}
                }
            }
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn trace(&self) -> &Trace {
        &self.trace
    }

    pub(crate) fn flow(&self) -> ControlFlow<()> {
        self.flow
    }

    pub(crate) fn take_shell_effects(&mut self) -> Vec<ShellEffect> {
        mem::take(&mut self.shell_effects)
    }

    pub(crate) fn frame(&self, now: Instant) -> Frame<'_> {
        Frame {
            model: &self.model,
            spectrum: &self.wiring.spectrum,
            latest: &self.wiring.cells,
            now: self.moment_of(now),
        }
    }

    pub(crate) fn now(&self) -> Moment {
        self.moment_of(Instant::now())
    }

    fn moment_of(&self, instant: Instant) -> Moment {
        Moment::new(self.unix_offset + instant.saturating_duration_since(self.epoch))
    }

    pub(crate) fn instant_of(&self, moment: Moment) -> Instant {
        self.epoch + moment.since_epoch().saturating_sub(self.unix_offset)
    }

    pub(crate) fn drain(self) {
        let Self {
            model,
            wiring,
            mut trace,
            ..
        } = self;
        let Wiring {
            mailbox, mut ports, ..
        } = wiring;
        ports.hang_up();
        let reported = await_exits(&model, &mailbox, Self::DRAIN);
        join_exited(&mut ports, &reported, &mut trace);
    }

    fn update(&mut self, message: Message) -> Result<Vec<Message>, Unhandled> {
        let now = self.now();
        let effects = kernel::update::update(&mut self.model, message, now)?;
        Ok(self.interpret(effects))
    }

    fn interpret(&mut self, effects: Vec<Effect>) -> Vec<Message> {
        let mut answers = Vec::new();
        let mut pending = effects;
        loop {
            let mut interpreter = Interpreter {
                drivers: &self.model.drivers,
                ports: &self.wiring.ports,
                timers: &mut self.timers,
                trace: &mut self.trace,
            };
            let interpreted = interpret(pending, &mut interpreter);
            self.shell_effects.extend(interpreted.shell_effects);
            answers.extend(interpreted.answers);
            if interpreted.flow.is_break() {
                self.flow = ControlFlow::Break(());
            }
            let Some((driver, rest)) = interpreted.restart else {
                break;
            };
            for entry in self.wiring.restart(driver, &self.model) {
                self.trace.push(entry);
            }
            pending = rest;
        }
        answers
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        convert::Infallible,
        path::Path,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use audio::tap::SpectrumTap;
    use config::driver::paths::{ConfigPaths, SeenTexts};
    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        cmd::{AudioCmd, LibraryCmd},
        domain::{
            direction::Direction,
            driver::{DriverName, DriverStatus, Restarts},
            setting_row::SettingRow,
            startup::Startup,
            toast::Toast,
        },
        message::{DriverEvent, Message},
        update::machine::Unhandled,
    };
    use library::dirs::LibraryDirs;
    use rstest::rstest;

    use crate::{
        driver_thread::{Congestion, DriverThread},
        error::Error,
        event_loop::run,
        runtime::Runtime,
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
        spawn::{Spawners, config_thread::spawn_config, tests::spawn_audio_loop},
        spawn_setup::SpawnSetup,
        startup_paths::StartupPaths,
        trace::Trace,
        wiring::Wiring,
    };

    fn boom() -> ! {
        panic!("boom")
    }

    fn stock_startup() -> Startup {
        Startup::default()
    }

    fn start_paths(directory: &Path) -> StartupPaths {
        StartupPaths {
            config: ConfigPaths {
                config: directory.join("config.toml"),
                appearance: directory.join("sifr-ui.toml"),
                themes: directory.join("themes"),
                theme: None,
                seen: SeenTexts::default(),
            },
            library: LibraryDirs::under(directory),
        }
    }

    struct QuitShell;

    impl Shell for QuitShell {
        type Input = ();
        type Error = Infallible;

        fn input(&mut self, (): ()) -> Reaction {
            Reaction::Message(Message::Quit)
        }

        fn effect(&mut self, _effect: ShellEffect) {}

        fn frame_due(&self, _frame: &Frame<'_>) -> FrameDue {
            FrameDue::Settled
        }

        fn paint(&mut self, _frame: Frame<'_>) -> Result<Painted, Infallible> {
            Ok(Painted::default())
        }
    }

    thread_local! {
        static AUDIO_TAP: RefCell<Option<Sender<AudioCmd>>> = const { RefCell::new(None) };
    }

    fn recording_audio(
        spawn_parts: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        let forward = AUDIO_TAP.with(|tap| tap.borrow().clone()).unwrap();
        spawn_audio_loop(
            move |inbox: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| {
                while let Ok(command) = inbox.recv() {
                    if forward.send(command).is_err() {
                        return;
                    }
                }
            },
            spawn_parts,
        )
    }

    fn recording_spawners() -> (Spawners, Receiver<AudioCmd>) {
        let (forward, commands) = unbounded();
        AUDIO_TAP.with(|tap| *tap.borrow_mut() = Some(forward));
        (
            Spawners {
                audio: recording_audio,
                ..Spawners::idle()
            },
            commands,
        )
    }

    fn panicking_audio(
        spawn_parts: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        spawn_audio_loop(
            |_: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| boom(),
            spawn_parts,
        )
    }

    fn panicking_spawners() -> Spawners {
        Spawners {
            audio: panicking_audio,
            ..Spawners::idle()
        }
    }

    #[test]
    fn start_sends_the_startup_stop_and_list_devices_to_the_stub_audio_inbox() {
        let directory = tempfile::tempdir().unwrap();
        let startup = stock_startup();
        let (spawners, commands) = recording_spawners();
        let runtime =
            Runtime::start(startup, &start_paths(directory.path()), &spawners).unwrap();
        let (keys, input) = unbounded();
        keys.send(()).unwrap();
        let mut shell = QuitShell;

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        assert_eq!(commands.recv().unwrap(), AudioCmd::Stop);
        assert_eq!(commands.recv().unwrap(), AudioCmd::ListDevices);
    }

    #[derive(Debug, Clone, Copy)]
    enum SaveStep {
        Step,
        Quit,
    }

    struct StepThenQuit;

    impl Shell for StepThenQuit {
        type Input = SaveStep;
        type Error = Infallible;

        fn input(&mut self, event: SaveStep) -> Reaction {
            match event {
                SaveStep::Step => Reaction::Message(Message::Step {
                    row: SettingRow::ReplayGain,
                    direction: Direction::Next,
                }),
                SaveStep::Quit => Reaction::Message(Message::Quit),
            }
        }

        fn effect(&mut self, _effect: ShellEffect) {}

        fn frame_due(&self, _frame: &Frame<'_>) -> FrameDue {
            FrameDue::Settled
        }

        fn paint(&mut self, _frame: Frame<'_>) -> Result<Painted, Infallible> {
            Ok(Painted::default())
        }
    }

    #[test]
    fn drain_on_stop_writes_the_pending_config_save() {
        let directory = tempfile::tempdir().unwrap();
        let paths = start_paths(directory.path());
        let config_path = paths.config.config.clone();
        let startup = stock_startup();
        let spawners = Spawners {
            config: spawn_config,
            ..Spawners::idle()
        };
        let runtime = Runtime::start(startup, &paths, &spawners).unwrap();

        let (steps, input) = unbounded();
        steps.send(SaveStep::Step).unwrap();
        steps.send(SaveStep::Quit).unwrap();
        let mut shell = StepThenQuit;

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            text.contains("replay_gain"),
            "drain must flush the pending replay_gain save to disk"
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum LifeStep {
        Paint,
        Quit,
    }

    struct ObserveDeadThenQuit {
        steps: Sender<LifeStep>,
        paints: usize,
        restarts: Restarts,
    }

    impl Shell for ObserveDeadThenQuit {
        type Input = LifeStep;
        type Error = Infallible;

        fn input(&mut self, event: LifeStep) -> Reaction {
            match event {
                LifeStep::Paint => Reaction::Ignored,
                LifeStep::Quit => Reaction::Message(Message::Quit),
            }
        }

        fn effect(&mut self, _effect: ShellEffect) {}

        fn frame_due(&self, _frame: &Frame<'_>) -> FrameDue {
            FrameDue::Settled
        }

        fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Infallible> {
            self.paints += 1;
            self.restarts = frame
                .model
                .drivers
                .record(DriverName::Audio)
                .restarts
                .clone();
            let next = if self.restarts != Restarts::default() || self.paints >= 20 {
                LifeStep::Quit
            } else {
                LifeStep::Paint
            };
            self.steps
                .send(next)
                .expect("the step receiver outlives the shell");
            Ok(Painted::default())
        }
    }

    #[test]
    fn a_driver_panic_is_supervised_through_frame() {
        let directory = tempfile::tempdir().unwrap();
        let startup = stock_startup();
        let runtime = Runtime::start(
            startup,
            &start_paths(directory.path()),
            &panicking_spawners(),
        )
        .unwrap();

        let (steps, input) = unbounded();
        steps.send(LifeStep::Paint).unwrap();
        let mut shell = ObserveDeadThenQuit {
            steps,
            paints: 0,
            restarts: Restarts::default(),
        };

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        assert_ne!(
            shell.restarts,
            Restarts::default(),
            "audio's standard supervision restarts a panicked driver"
        );
    }

    fn start(library: DriverStatus) -> (Runtime, Receiver<LibraryCmd>) {
        let (wiring, library_inbox, _writers) = Wiring::idle();
        let started = kernel::update::startup::startup(stock_startup());
        let mut runtime = Runtime::assemble(started, wiring, Trace::default()).unwrap();
        runtime.model.drivers.record_mut(DriverName::Library).status = library;
        (runtime, library_inbox)
    }

    #[test]
    fn now_is_anchored_to_the_wall_clock_and_round_trips_through_instant_of() {
        let (runtime, _library_inbox) = start(DriverStatus::Running);

        let now = runtime.now();

        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let apart = wall.abs_diff(now.since_epoch());
        assert!(
            apart < Duration::from_secs(5),
            "now is {apart:?} off the wall clock"
        );
        let back = runtime.moment_of(runtime.instant_of(now));
        assert_eq!(back, now);
        runtime.drain();
    }

    #[rstest]
    #[case::an_accepted_message_applies(
        DriverStatus::Running,
        Message::Toast(Toast::error("hello".to_owned())),
        Ok(())
    )]
    #[case::a_rejected_message_is_refused(
        DriverStatus::Stopped,
        Message::Driver { driver: DriverName::Library, event: DriverEvent::Stopped },
        Err(Unhandled)
    )]
    fn step_reports_whether_the_message_changed_the_model(
        #[case] library: DriverStatus,
        #[case] message: Message,
        #[case] expected: Result<(), Unhandled>,
    ) {
        let (mut runtime, _library_inbox) = start(library);

        let change = runtime.deliver(message);

        assert_eq!(change, expected);
        runtime.drain();
    }

    #[test]
    fn runtime_is_send() {
        fn sendable<T: Send>() {}
        sendable::<Runtime>();
    }
}
