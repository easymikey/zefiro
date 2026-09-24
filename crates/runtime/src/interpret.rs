use std::time::Instant;

use config::AppearancePatch;
use crossbeam_channel::Sender;
use kernel::{
    AudioCmd,
    Cmd,
    ConfigCmd,
    ConfigPatch,
    Effect,
    LibraryCmd,
    LoadedRequest,
    Message,
    SystemCmd,
    domain::{Driver, DriverStatus, Drivers},
};
use strum::IntoStaticStr;

use crate::{
    shell::{Flow, ShellEffect},
    timers::Timers,
    trace::{Trace, TraceEntry},
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ConfigCommand {
    Save(ConfigPatch),
    SelectTheme(String),
    Appearance(AppearancePatch),
}

impl From<ConfigCmd> for ConfigCommand {
    fn from(command: ConfigCmd) -> Self {
        match command {
            ConfigCmd::Save(patch) => ConfigCommand::Save(patch),
            ConfigCmd::SelectTheme(name) => ConfigCommand::SelectTheme(name),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Interpreted {
    pub answers: Vec<Message>,
    pub shell: Vec<ShellEffect>,
    pub flow: Flow,
}

#[derive(Debug)]
pub(crate) struct Interpreter<'a> {
    pub drivers: &'a Drivers,
    pub audio: &'a Sender<AudioCmd>,
    pub library: &'a Sender<LibraryCmd>,
    pub config: &'a Sender<ConfigCommand>,
    pub macos: Option<&'a Sender<SystemCmd>>,
    pub timers: &'a mut Timers,
    pub trace: &'a mut Trace,
}

pub(crate) struct Dropped;

pub(crate) struct Target<'a, C> {
    pub driver: Driver,
    pub sender: &'a Sender<C>,
}

impl<C> Clone for Target<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for Target<'_, C> {}

pub(crate) fn gated<C>(
    drivers: &Drivers,
    target: Target<'_, C>,
    command: C,
) -> Result<(), Dropped> {
    let running = matches!(drivers.status(target.driver), DriverStatus::Running);
    if running && target.sender.send(command).is_ok() {
        Ok(())
    } else {
        Err(Dropped)
    }
}

impl Interpreter<'_> {
    fn dropped(&mut self, driver: Driver, command: &'static str) {
        self.trace.push(TraceEntry::Dropped { driver, command });
    }

    fn send_audio(&mut self, command: AudioCmd) {
        let label: &'static str = (&command).into();
        let target = Target {
            driver: Driver::Audio,
            sender: self.audio,
        };
        if gated(self.drivers, target, command).is_err() {
            self.dropped(Driver::Audio, label);
        }
    }

    fn send_library(&mut self, command: LibraryCmd) {
        let label: &'static str = (&command).into();
        let target = Target {
            driver: Driver::Library,
            sender: self.library,
        };
        if gated(self.drivers, target, command).is_err() {
            self.dropped(Driver::Library, label);
        }
    }

    fn send_config(&mut self, command: ConfigCommand) {
        let label: &'static str = (&command).into();
        let target = Target {
            driver: Driver::Config,
            sender: self.config,
        };
        if gated(self.drivers, target, command).is_err() {
            self.dropped(Driver::Config, label);
        }
    }

    fn send_macos(&mut self, command: SystemCmd) {
        let Some(sender) = self.macos else {
            return;
        };
        let label: &'static str = (&command).into();
        let target = Target {
            driver: Driver::Macos,
            sender,
        };
        if gated(self.drivers, target, command).is_err() {
            self.dropped(Driver::Macos, label);
        }
    }
}

fn shuffle_order(len: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    fastrand::shuffle(&mut order);
    order
}

pub(crate) fn interpret(cmd: Cmd, runtime: &mut Interpreter<'_>) -> Interpreted {
    let mut interpreted = Interpreted::default();
    for effect in cmd {
        match effect {
            Effect::Audio(command) => runtime.send_audio(command),
            Effect::Library(command) => runtime.send_library(command),
            Effect::System(command) => runtime.send_macos(command),
            Effect::Config(command) => runtime.send_config(command.into()),
            Effect::WindowColors(command) => {
                interpreted.shell.push(ShellEffect::WindowColors(command));
            }
            Effect::Animate(cue) => interpreted.shell.push(ShellEffect::Animate(cue)),
            Effect::RollShuffle { len } => {
                interpreted.answers.push(Message::Loaded(
                    LoadedRequest::ShuffleRolled(shuffle_order(len)),
                ));
            }
            Effect::Setting { id, position } => {
                match config::appearance_patch(id, position) {
                    Ok(patch) => {
                        runtime.send_config(ConfigCommand::Appearance(patch));
                        interpreted.shell.push(ShellEffect::Appearance(patch));
                    }
                    Err(rejection) => {
                        runtime.trace.push(TraceEntry::SettingRejected(rejection));
                    }
                }
            }
            Effect::After { delay, message } => {
                if let Some(deadline) = Instant::now().checked_add(delay) {
                    runtime.timers.schedule(deadline, message);
                } else {
                    let timer: &'static str = (&message).into();
                    runtime.trace.push(TraceEntry::TimerOverflow { timer });
                }
            }
            Effect::Quit => interpreted.flow = Flow::Stop,
        }
    }
    interpreted
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::{Receiver, Sender, never, unbounded};
    use kernel::{
        AudioCmd,
        Cmd,
        ConfigCmd,
        ConfigPatch,
        Cue,
        Effect,
        LibraryCmd,
        LoadedRequest,
        Message,
        SystemCmd,
        Timer,
        WindowColorsCmd,
        domain::{Driver, DriverStatus, Drivers, Model, Revision, SettingId},
    };
    use rstest::rstest;

    use crate::{
        interpret::{ConfigCommand, Interpreter, Target, gated, interpret},
        shell::{Flow, ShellEffect},
        timers::Timers,
        trace::{Trace, TraceEntry},
    };

    #[rstest]
    #[case::running_and_connected(DriverStatus::Running, true, true)]
    #[case::stopped_and_connected(DriverStatus::Stopped, true, false)]
    #[case::running_and_disconnected(DriverStatus::Running, false, false)]
    fn gated_sends_only_to_a_running_and_connected_driver(
        #[case] status: DriverStatus,
        #[case] connected: bool,
        #[case] expects_delivery: bool,
    ) {
        let (audio, audio_rx) = unbounded();
        if !connected {
            drop(audio_rx);
        }
        let drivers = Drivers {
            audio: status,
            ..Drivers::default()
        };
        let target = Target {
            driver: Driver::Audio,
            sender: &audio,
        };

        let sent = gated(&drivers, target, AudioCmd::Stop);

        assert_eq!(sent.is_ok(), expects_delivery);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum MacosLink {
        Present,
        Absent,
    }

    struct Fixture {
        model: Model,
        audio_tx: Sender<AudioCmd>,
        audio_rx: Receiver<AudioCmd>,
        library_tx: Sender<LibraryCmd>,
        library_rx: Receiver<LibraryCmd>,
        config_tx: Sender<ConfigCommand>,
        config_rx: Receiver<ConfigCommand>,
        macos_tx: Sender<SystemCmd>,
        macos_rx: Receiver<SystemCmd>,
        timers: Timers,
        trace: Trace,
    }

    impl Fixture {
        fn new() -> Self {
            let (audio_tx, audio_rx) = unbounded();
            let (library_tx, library_rx) = unbounded();
            let (config_tx, config_rx) = unbounded();
            let (macos_tx, macos_rx) = unbounded();
            Self {
                model: Model::default(),
                audio_tx,
                audio_rx,
                library_tx,
                library_rx,
                config_tx,
                config_rx,
                macos_tx,
                macos_rx,
                timers: Timers::default(),
                trace: Trace::default(),
            }
        }

        fn interpreter(&mut self, macos: MacosLink) -> Interpreter<'_> {
            Interpreter {
                drivers: &self.model.drivers,
                audio: &self.audio_tx,
                library: &self.library_tx,
                config: &self.config_tx,
                macos: match macos {
                    MacosLink::Present => Some(&self.macos_tx),
                    MacosLink::Absent => None,
                },
                timers: &mut self.timers,
                trace: &mut self.trace,
            }
        }
    }

    #[test]
    fn a_command_to_a_running_driver_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(Cmd::One(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
        assert!(fixture.trace.is_empty());
    }

    #[test]
    fn a_command_to_a_dead_driver_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.model.drivers.audio = DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(Cmd::One(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert!(fixture.audio_rx.try_recv().is_err());
        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Audio,
                command: "stop"
            })
        );
    }

    #[test]
    fn a_send_onto_a_lost_inbox_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.audio_rx = never();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(Cmd::One(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Audio,
                command: "stop"
            })
        );
    }

    #[test]
    fn a_system_effect_on_a_platform_without_the_macos_driver_records_nothing() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(
            Cmd::One(Effect::System(
                SystemCmd::Volume(kernel::Percent::default()),
            )),
            &mut interpreter,
        );

        assert!(fixture.trace.is_empty());
    }

    #[test]
    fn a_system_command_to_a_stopped_macos_driver_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.model.drivers.macos = DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter(MacosLink::Present);

        interpret(
            Cmd::One(Effect::System(
                SystemCmd::Volume(kernel::Percent::default()),
            )),
            &mut interpreter,
        );

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Macos,
                command: "volume"
            })
        );
    }

    #[test]
    fn a_system_send_onto_a_lost_macos_inbox_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.macos_rx = never();
        let mut interpreter = fixture.interpreter(MacosLink::Present);

        interpret(
            Cmd::One(Effect::System(
                SystemCmd::Volume(kernel::Percent::default()),
            )),
            &mut interpreter,
        );

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Macos,
                command: "volume"
            })
        );
    }

    #[test]
    fn a_library_command_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(
            Cmd::One(Effect::Library(LibraryCmd::LoadFavorites)),
            &mut interpreter,
        );

        assert_eq!(fixture.library_rx.try_recv(), Ok(LibraryCmd::LoadFavorites));
    }

    #[test]
    fn a_config_save_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(
            Cmd::One(Effect::Config(ConfigCmd::Save(
                ConfigPatch::builder().theme("dark").build(),
            ))),
            &mut interpreter,
        );

        assert!(matches!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCommand::Save(patch)) if patch.theme.as_deref() == Some("dark")
        ));
    }

    #[test]
    fn window_colors_and_animate_become_shell_effects() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        let interpreted = interpret(
            Cmd::Batch(vec![
                Effect::WindowColors(WindowColorsCmd::Reset),
                Effect::Animate(Cue::TrackChanged),
            ]),
            &mut interpreter,
        );

        assert_eq!(
            interpreted.shell,
            vec![
                ShellEffect::WindowColors(WindowColorsCmd::Reset),
                ShellEffect::Animate(Cue::TrackChanged),
            ]
        );
    }

    #[test]
    fn quit_stops_the_flow_after_the_rest_of_the_batch() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        let interpreted = interpret(
            Cmd::Batch(vec![Effect::Audio(AudioCmd::Stop), Effect::Quit]),
            &mut interpreter,
        );

        assert_eq!(interpreted.flow, Flow::Stop);
        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
    }

    #[test]
    fn roll_shuffle_answers_with_a_permutation_of_the_right_length() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        let interpreted =
            interpret(Cmd::One(Effect::RollShuffle { len: 5 }), &mut interpreter);
        let [Message::Loaded(LoadedRequest::ShuffleRolled(order))] =
            interpreted.answers.as_slice()
        else {
            panic!("expected a single shuffle answer");
        };
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn two_roll_shuffles_in_one_batch_answer_in_order() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        let interpreted = interpret(
            Cmd::Batch(vec![
                Effect::RollShuffle { len: 2 },
                Effect::RollShuffle { len: 3 },
            ]),
            &mut interpreter,
        );

        let [
            Message::Loaded(LoadedRequest::ShuffleRolled(first)),
            Message::Loaded(LoadedRequest::ShuffleRolled(second)),
        ] = interpreted.answers.as_slice()
        else {
            panic!("expected two shuffle answers in order");
        };
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 3);
    }

    fn setting_id(field: config::AppearanceField) -> SettingId {
        config::APPEARANCE_ROWS[field as usize].id
    }

    #[test]
    fn a_setting_effect_reaches_the_config_inbox_and_the_shell_in_the_same_step() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        let interpreted = interpret(
            Cmd::One(Effect::Setting {
                id: setting_id(config::AppearanceField::CoverBrackets),
                position: 0,
            }),
            &mut interpreter,
        );

        assert!(matches!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCommand::Appearance(_))
        ));
        assert!(matches!(
            interpreted.shell.as_slice(),
            [ShellEffect::Appearance(_)]
        ));
    }

    #[test]
    fn an_unknown_setting_is_traced_and_never_reaches_the_config_inbox() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(
            Cmd::One(Effect::Setting {
                id: SettingId(u16::MAX),
                position: 0,
            }),
            &mut interpreter,
        );

        assert!(fixture.config_rx.try_recv().is_err());
        assert!(matches!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::SettingRejected(_))
        ));
    }

    #[test]
    fn after_schedules_a_timer() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(
            Cmd::One(Effect::After {
                delay: Duration::from_secs(1),
                message: Timer::Toast(Revision::default()),
            }),
            &mut interpreter,
        );

        assert!(fixture.timers.next_deadline().is_some());
    }

    #[test]
    fn a_delay_that_would_overflow_the_clock_is_traced_and_skipped() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter(MacosLink::Absent);

        interpret(
            Cmd::One(Effect::After {
                delay: Duration::MAX,
                message: Timer::Toast(Revision::default()),
            }),
            &mut interpreter,
        );

        assert!(fixture.timers.next_deadline().is_none());
        assert!(matches!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::TimerOverflow { timer: "toast" })
        ));
    }
}
