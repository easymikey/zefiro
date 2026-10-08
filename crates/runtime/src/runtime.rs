use std::{
    collections::VecDeque,
    mem,
    ops::ControlFlow,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use kernel::{
    cmd::Effect,
    domain::{model::Model, startup::Startup, time::Moment},
    message::{DriverEvent, Message, Timer},
    update::machine::Unhandled,
};

use crate::{
    error::{ClockError, Error},
    registry,
    shell::{Frame, ShellEffect},
    spawn::Spawners,
    spawn_setup::StartupPaths,
    timers::Timers,
    wiring::{Wiring, await_exits, join_exited},
};

#[derive(Debug)]
pub struct Runtime {
    pub(crate) model: Model,
    pub(crate) wiring: Wiring,
    pub(crate) timers: Timers<Timer>,
    started_at: Instant,
    unix_offset: Duration,
    pub(crate) flow: ControlFlow<()>,
    pub(crate) shell_effects: Vec<ShellEffect>,
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
        Ok(Self::assemble(model, effects, wiring)?)
    }

    pub(crate) fn assemble(
        model: Model,
        effects: Vec<Effect>,
        wiring: Wiring,
    ) -> Result<Self, ClockError> {
        let mut runtime = Self {
            model,
            wiring,
            timers: Timers::default(),
            started_at: Instant::now(),
            unix_offset: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(ClockError)?,
            flow: ControlFlow::Continue(()),
            shell_effects: Vec::new(),
        };
        let stopped = registry::REGISTRY
            .iter()
            .filter(|row| !row.platform.is_present())
            .map(|row| Message::Driver {
                driver_name: row.driver_name,
                event: DriverEvent::Stopped,
            });
        for message in runtime.interpret(effects).into_iter().chain(stopped) {
            match runtime.deliver(message) {
                Ok(()) | Err(Unhandled) => {}
            }
        }
        Ok(runtime)
    }

    pub(crate) fn deliver(&mut self, message: Message) -> Result<(), Unhandled> {
        let following = self.update(message)?;
        if following.is_empty() {
            return Ok(());
        }
        let mut queue = VecDeque::from(following);
        while let Some(current) = queue.pop_front() {
            if let Ok(next) = self.update(current) {
                queue.extend(next);
            }
        }
        Ok(())
    }

    pub(crate) fn take_shell_effects(&mut self) -> Vec<ShellEffect> {
        mem::take(&mut self.shell_effects)
    }

    pub(crate) fn frame(&self, now: Instant) -> Frame<'_> {
        Frame {
            model: &self.model,
            spectrum_tap: &self.wiring.spectrum_tap,
            latest_receivers: &self.wiring.latest_receivers,
            now: self.moment_of(now),
        }
    }

    pub(crate) fn now(&self) -> Moment {
        self.moment_of(Instant::now())
    }

    fn moment_of(&self, instant: Instant) -> Moment {
        Moment::new(
            self.unix_offset + instant.saturating_duration_since(self.started_at),
        )
    }

    pub(crate) fn instant_of(&self, moment: Moment) -> Instant {
        self.started_at + moment.since_epoch().saturating_sub(self.unix_offset)
    }

    pub(crate) fn drain(self) {
        let Self {
            model,
            wiring,
            timers: _timers,
            started_at: _started_at,
            unix_offset: _unix_offset,
            flow: _flow,
            shell_effects: _shell_effects,
        } = self;
        let Wiring {
            inbox_receiver,
            inbox: _inbox,
            mut ports,
            spectrum_tap: _spectrum_tap,
            latest_receivers: _latest_receivers,
            doorbell: _doorbell,
            spawners: _spawners,
            paths: _paths,
            latest_senders: _latest_senders,
            #[cfg(target_os = "macos")]
                macos_channel: _macos_channel,
        } = wiring;
        for row in registry::REGISTRY {
            ports.hang_up(row.driver_name);
        }
        let reported = await_exits(&model, &inbox_receiver, Self::DRAIN);
        join_exited(&mut ports, &reported);
    }

    fn update(&mut self, message: Message) -> Result<Vec<Message>, Unhandled> {
        let now = self.now();
        let effects = kernel::update::update(&mut self.model, message, now)?;
        Ok(self.interpret(effects))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        convert::Infallible,
        io,
        sync::atomic::{AtomicUsize, Ordering},
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use audio::tap::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        cmd::{AudioCmd, LibraryCmd},
        domain::{
            direction::Direction,
            driver::{DriverError, DriverName, DriverStatus, Restarts},
            setting_row::SettingRow,
            startup::Startup,
            toast::Toast,
        },
        message::{DriverEvent, Message},
        update::machine::Unhandled,
    };
    use rstest::rstest;

    use crate::{
        driver::tests::{LONG_JOB, RECV_TIMEOUT},
        driver_thread::{Congestion, DriverThread},
        error::{Error, SpawnError},
        event_loop::run,
        runtime::Runtime,
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
        spawn::{
            Spawners,
            config_thread::spawn_config,
            tests::{boom, idle_spawners, spawn_audio_loop, stub_paths},
        },
        spawn_setup::SpawnSetup,
        wiring::Wiring,
    };

    fn stock_startup() -> Startup {
        Startup::default()
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
        static AUDIO_CMD_SENDER: RefCell<Option<Sender<AudioCmd>>> = const { RefCell::new(None) };
    }

    fn recording_audio(
        spawn_setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        let cmd_sender = AUDIO_CMD_SENDER
            .with(|cmd_sender_cell| cmd_sender_cell.borrow().clone())
            .unwrap();
        spawn_audio_loop(
            move |cmd_receiver: &Receiver<AudioCmd>,
                  _: &Sender<Message>,
                  _: &Congestion| {
                while let Ok(cmd) = cmd_receiver.recv() {
                    if cmd_sender.send(cmd).is_err() {
                        return Ok(());
                    }
                }
                Ok(())
            },
            spawn_setup,
        )
    }

    fn recording_spawners() -> (Spawners, Receiver<AudioCmd>) {
        let (cmd_sender, cmd_receiver) = unbounded();
        AUDIO_CMD_SENDER
            .with(|cmd_sender_cell| *cmd_sender_cell.borrow_mut() = Some(cmd_sender));
        (
            Spawners {
                audio: recording_audio,
                ..idle_spawners()
            },
            cmd_receiver,
        )
    }

    fn panicking_audio(
        spawn_setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        spawn_audio_loop(
            |_: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| boom(),
            spawn_setup,
        )
    }

    fn panicking_spawners() -> Spawners {
        Spawners {
            audio: panicking_audio,
            ..idle_spawners()
        }
    }

    #[test]
    fn start_sends_the_startup_stop_and_list_devices_to_the_stub_audio_cmd_receiver() {
        let directory = tempfile::tempdir().unwrap();
        let startup = stock_startup();
        let (spawners, cmd_receiver) = recording_spawners();
        let runtime =
            Runtime::start(startup, &stub_paths(directory.path()), &spawners).unwrap();
        let (keys, input) = unbounded();
        keys.send(()).unwrap();
        let mut shell = QuitShell;

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        assert_eq!(cmd_receiver.recv().unwrap(), AudioCmd::Stop);
        assert_eq!(cmd_receiver.recv().unwrap(), AudioCmd::ListDevices);
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

        fn input(&mut self, save_step: SaveStep) -> Reaction {
            match save_step {
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
        let paths = stub_paths(directory.path());
        let config_path = paths.config_paths.config_path.clone();
        let startup = stock_startup();
        let spawners = Spawners {
            config: spawn_config,
            ..idle_spawners()
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

    struct StepThenPaintError;

    impl Shell for StepThenPaintError {
        type Input = SaveStep;
        type Error = io::Error;

        fn input(&mut self, save_step: SaveStep) -> Reaction {
            match save_step {
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

        fn paint(&mut self, _frame: Frame<'_>) -> Result<Painted, io::Error> {
            Err(io::Error::other("broken pipe"))
        }
    }

    #[test]
    fn a_paint_failure_still_writes_the_pending_config_save() {
        let directory = tempfile::tempdir().unwrap();
        let paths = stub_paths(directory.path());
        let config_path = paths.config_paths.config_path.clone();
        let startup = stock_startup();
        let spawners = Spawners {
            config: spawn_config,
            ..idle_spawners()
        };
        let runtime = Runtime::start(startup, &paths, &spawners).unwrap();

        let (steps, input) = unbounded();
        steps.send(SaveStep::Step).unwrap();
        let mut shell = StepThenPaintError;

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Err(Error::Paint(_))));
        assert!(
            std::fs::read_to_string(&config_path)
                .is_ok_and(|text| text.contains("replay_gain")),
            "a failed run must still flush the pending replay_gain save to disk"
        );
    }

    #[test]
    fn drain_gives_up_on_a_driver_that_never_reports_its_stop() {
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: |spawn_setup| {
                spawn_audio_loop(
                    |_: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| {
                        thread::sleep(LONG_JOB);
                        Ok(())
                    },
                    spawn_setup,
                )
            },
            ..idle_spawners()
        };
        let asked = Instant::now();
        let runtime =
            Runtime::start(stock_startup(), &stub_paths(directory.path()), &spawners)
                .unwrap();
        let (keys, input) = unbounded();
        keys.send(()).unwrap();
        let mut shell = QuitShell;

        let ended = run(runtime, &mut shell, &input);

        assert!(matches!(ended, Ok(())));
        assert!(asked.elapsed() < Runtime::DRAIN + RECV_TIMEOUT);
    }

    #[derive(Debug, Clone, Copy)]
    enum LifeStep {
        Paint,
        Quit,
    }

    struct ObserveDeadThenQuit {
        step_sender: Sender<LifeStep>,
        paints: usize,
        restarts: Restarts,
    }

    impl Shell for ObserveDeadThenQuit {
        type Input = LifeStep;
        type Error = Infallible;

        fn input(&mut self, life_step: LifeStep) -> Reaction {
            match life_step {
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
            self.step_sender
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
            &stub_paths(directory.path()),
            &panicking_spawners(),
        )
        .unwrap();

        let (steps, input) = unbounded();
        steps.send(LifeStep::Paint).unwrap();
        let mut shell_quit = ObserveDeadThenQuit {
            step_sender: steps,
            paints: 0,
            restarts: Restarts::default(),
        };

        let ended = run(runtime, &mut shell_quit, &input);

        assert!(matches!(ended, Ok(())));
        assert_ne!(
            shell_quit.restarts,
            Restarts::default(),
            "audio's standard supervision restarts a panicked driver"
        );
    }

    static AUDIO_SPAWNS: AtomicUsize = AtomicUsize::new(0);

    fn audio_that_cannot_restart(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        if AUDIO_SPAWNS.fetch_add(1, Ordering::SeqCst) == 0 {
            panicking_audio(setup)
        } else {
            Err(SpawnError::Thread {
                driver_name: DriverName::Audio,
                error: io::Error::other("no threads left"),
            })
        }
    }

    #[test]
    fn a_restart_that_fails_to_spawn_leaves_the_driver_died() {
        AUDIO_SPAWNS.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: audio_that_cannot_restart,
            ..idle_spawners()
        };
        let mut runtime =
            Runtime::start(stock_startup(), &stub_paths(directory.path()), &spawners)
                .unwrap();

        let died = runtime
            .wiring
            .inbox_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        runtime.deliver(died).unwrap();

        assert_eq!(
            runtime.model.drivers.status(DriverName::Audio),
            &DriverStatus::Dead(DriverError::Spawn {
                error: kernel::domain::io_error::IoError::Other
            })
        );
        runtime.drain();
    }

    fn start(driver_status: DriverStatus) -> (Runtime, Receiver<LibraryCmd>) {
        let (wiring, library_cmd_receiver, _latest_senders) = Wiring::idle();
        let (model, effects) = kernel::update::startup::startup(stock_startup());
        let mut runtime = Runtime::assemble(model, effects, wiring).unwrap();
        runtime.model.drivers.record_mut(DriverName::Library).status = driver_status;
        (runtime, library_cmd_receiver)
    }

    #[test]
    fn now_is_anchored_to_the_wall_clock_and_round_trips_through_instant_of() {
        let (runtime, _library_cmd_receiver) = start(DriverStatus::Running);

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
        Message::Driver { driver_name: DriverName::Library, event: DriverEvent::Stopped },
        Err(Unhandled)
    )]
    fn step_reports_whether_the_message_changed_the_model(
        #[case] driver_status: DriverStatus,
        #[case] message: Message,
        #[case] expected: Result<(), Unhandled>,
    ) {
        let (mut runtime, _library_cmd_receiver) = start(driver_status);

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
