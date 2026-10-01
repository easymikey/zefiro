use std::{
    collections::VecDeque,
    mem,
    ops::ControlFlow,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::Sender;
use kernel::{
    Cmd,
    DriverMessage,
    Message,
    Moment,
    domain::{Model, Startup},
};
use library::LibraryDirs;

use crate::{
    config::ConfigPaths,
    error::Error,
    interpret::{Interpreter, interpret},
    library::cover::CoverRequest,
    registry,
    shell::{Frame, ShellEffect},
    spawn::Spawners,
    timers::Timers,
    trace::{Trace, TraceEntry},
    wiring::{Wiring, await_exits, join_exited},
};

#[derive(Debug, Clone)]
pub struct StartupPaths {
    pub config: ConfigPaths,
    pub library: LibraryDirs,
}

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
    cover: Option<CoverRequest>,
    full_episodes: u32,
}

pub(crate) struct Seed {
    model: Model,
    cmd: Cmd,
}

impl Runtime {
    pub(crate) const DRAIN: Duration = Duration::from_secs(2);

    pub fn start(
        startup: Startup,
        paths: &StartupPaths,
        spawners: &Spawners,
    ) -> Result<Self, Error> {
        let seed = Self::seeded(startup);
        let wiring = Wiring::spawn(&seed.model, paths, spawners)?;
        Ok(Self::assemble(seed, wiring, Trace::default()))
    }

    pub(crate) fn seeded(startup: Startup) -> Seed {
        let (model, cmd) = kernel::startup(startup);
        Seed { model, cmd }
    }

    pub(crate) fn assemble(seed: Seed, wiring: Wiring, trace: Trace) -> Self {
        let Seed { model, cmd } = seed;
        let mut runtime = Self {
            model,
            wiring,
            timers: Timers::default(),
            trace,
            epoch: Instant::now(),
            unix_offset: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO),
            flow: ControlFlow::Continue(()),
            shell_effects: Vec::new(),
            cover: None,
            full_episodes: 0,
        };
        for answer in runtime.interpret(cmd) {
            runtime.step(answer);
        }
        for row in registry::REGISTRY {
            if !row.platform.present() {
                runtime.step(Message::Driver {
                    driver: row.driver,
                    event: DriverMessage::Stopped,
                });
            }
        }
        runtime
    }

    pub(crate) fn step(&mut self, message: Message) -> bool {
        let mut queue: VecDeque<Message> = VecDeque::from([message]);
        let mut applied = false;
        while let Some(current) = queue.pop_front() {
            if let Some(following) = self.update(current) {
                applied = true;
                queue.extend(following);
            }
        }
        applied
    }

    pub(crate) fn report_full(&mut self) {
        for row in registry::REGISTRY {
            if self.flow.is_break() {
                return;
            }
            if self.wiring.ports.full_edge(row.driver).take() {
                self.full_episodes += 1;
                self.step(Message::Driver {
                    driver: row.driver,
                    event: DriverMessage::Full,
                });
            }
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn trace(&self) -> &Trace {
        &self.trace
    }

    #[must_use]
    pub(crate) fn sleep_deadline(&self) -> Option<Moment> {
        self.timers
            .sleep_deadline(self.model.transport.sleep)
            .map(|deadline| self.moment_of(deadline))
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
            sleep_deadline: self.sleep_deadline(),
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

    pub(crate) fn request_cover(&mut self, request: CoverRequest) {
        if self.cover.as_ref() == Some(&request) {
            return;
        }
        let sent = self
            .wiring
            .ports
            .library
            .send_cover(&self.model.drivers, request.clone());
        self.cover = Some(request);
        self.trace.record(sent);
    }

    #[must_use]
    pub(crate) fn sender(&self) -> Sender<Message> {
        self.wiring.sender.clone()
    }

    pub(crate) fn drain(self) {
        let Self {
            model,
            wiring,
            mut trace,
            ..
        } = self;
        let Wiring {
            receiver,
            mut ports,
            ..
        } = wiring;
        ports.hang_up();
        let reported = await_exits(&model, &receiver, Self::DRAIN);
        join_exited(&mut ports, &reported, &mut trace);
    }

    fn update(&mut self, message: Message) -> Option<Vec<Message>> {
        let label: &'static str = (&message).into();
        let now = self.now();
        match kernel::update::update(&mut self.model, message, now) {
            Ok(cmd) => Some(self.interpret(cmd)),
            Err(error) => {
                self.trace.push(TraceEntry::Rejected {
                    message: label,
                    error,
                });
                None
            }
        }
    }

    fn interpret(&mut self, cmd: Cmd) -> Vec<Message> {
        let mut answers = Vec::new();
        let mut pending = cmd;
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
            let Some((driver, effects)) = interpreted.restart else {
                break;
            };
            for entry in self.wiring.restart(driver, &self.model) {
                self.trace.push(entry);
            }
            pending = Cmd::Batch(effects);
        }
        answers
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        convert::Infallible,
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        AudioCmd,
        AudioEvent,
        Direction,
        DriverMessage,
        Message,
        Toast,
        domain::{Driver, DriverStatus, SettingRow, Startup},
    };
    use library::LibraryDirs;
    use rstest::rstest;

    use crate::{
        config::{ConfigPaths, SeenTexts},
        error::Error,
        event_loop::run,
        library::{cover::CoverRequest, machine::LibraryMessage},
        runtime::{Runtime, StartupPaths},
        sender::DriverSender,
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
        spawn::{AudioDriver, SpawnParts, Spawners, spawn_audio_loop, spawn_config},
        trace::{DropReason, Trace, TraceEntry},
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

    fn recording_audio(spawn_parts: &SpawnParts<'_>) -> Result<AudioDriver, Error> {
        let forward = AUDIO_TAP.with(|tap| tap.borrow().clone()).unwrap();
        spawn_audio_loop(
            move |inbox: &Receiver<AudioCmd>, _: &DriverSender<AudioEvent>| {
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

    fn panicking_audio(spawn_parts: &SpawnParts<'_>) -> Result<AudioDriver, Error> {
        spawn_audio_loop(
            |_: &Receiver<AudioCmd>, _: &DriverSender<AudioEvent>| boom(),
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
        Adjust,
        Quit,
    }

    struct AdjustThenQuit;

    impl Shell for AdjustThenQuit {
        type Input = SaveStep;
        type Error = Infallible;

        fn input(&mut self, event: SaveStep) -> Reaction {
            match event {
                SaveStep::Adjust => Reaction::Message(Message::Adjust {
                    row: SettingRow::Replaygain,
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
        steps.send(SaveStep::Adjust).unwrap();
        steps.send(SaveStep::Quit).unwrap();
        let mut shell = AdjustThenQuit;

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            text.contains("replaygain"),
            "drain must flush the pending replaygain save to disk"
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
        restarts: usize,
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
            self.restarts = frame.model.drivers.record(Driver::Audio).restarts.count();
            let next = if self.restarts > 0 || self.paints >= 20 {
                LifeStep::Quit
            } else {
                LifeStep::Paint
            };
            let _ = self.steps.send(next);
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
            restarts: 0,
        };

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        assert!(
            (1..=3).contains(&shell.restarts),
            "audio's standard supervision caps automatic restarts at 3 within 60s, saw {}",
            shell.restarts
        );
    }

    fn start(library: DriverStatus) -> (Runtime, Receiver<LibraryMessage>) {
        let (wiring, cover_inbox, _writers) = Wiring::idle();
        let seed = Runtime::seeded(stock_startup());
        let mut runtime = Runtime::assemble(seed, wiring, Trace::default());
        runtime.model.drivers.record_mut(Driver::Library).status = library;
        (runtime, cover_inbox)
    }

    #[test]
    fn now_is_anchored_to_the_wall_clock_and_round_trips_through_instant_of() {
        let (runtime, _cover_inbox) = start(DriverStatus::Running);

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

    #[test]
    fn a_refused_cover_request_is_traced_once_not_on_every_paint() {
        let (mut runtime, _cover_inbox) = start(DriverStatus::Stopped);
        let request = CoverRequest {
            path: PathBuf::from("/music/cover.jpg"),
            size_px: 64,
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
                        command: "cover",
                        reason: DropReason::NotRunning,
                    }
                )
            })
            .count();
        assert_eq!(dropped, 1);
        runtime.drain();
    }

    #[test]
    fn a_repeated_accepted_cover_request_is_sent_once() {
        let (mut runtime, cover_inbox) = start(DriverStatus::Running);
        let request = CoverRequest {
            path: PathBuf::from("/music/cover.jpg"),
            size_px: 64,
        };

        runtime.request_cover(request.clone());
        runtime.request_cover(request);

        assert!(runtime.trace.is_empty());
        runtime.drain();
        let covers = cover_inbox
            .iter()
            .filter(|command| matches!(command, LibraryMessage::Cover(_)))
            .count();
        assert_eq!(covers, 1);
    }

    #[rstest]
    #[case::an_accepted_message_applies(
        DriverStatus::Running,
        Message::Toast(Toast::error("hello".to_owned())),
        true
    )]
    #[case::a_rejected_message_is_refused(
        DriverStatus::Stopped,
        Message::Driver { driver: Driver::Library, event: DriverMessage::Stopped },
        false
    )]
    fn step_reports_whether_the_message_changed_the_model(
        #[case] library: DriverStatus,
        #[case] message: Message,
        #[case] expected: bool,
    ) {
        let (mut runtime, _cover_inbox) = start(library);

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
