use std::{ops::ControlFlow, time::Instant};

use kernel::{
    AudioCmd,
    Cmd,
    ConfigCmd,
    Effect,
    LibraryCmd,
    MacosCmd,
    Message,
    PlaylistRequest,
    domain::{Driver, Drivers},
};

use crate::{
    port::Ports,
    shell::ShellEffect,
    timers::Timers,
    trace::{Trace, TraceEntry},
};

#[derive(Debug, PartialEq)]
pub(crate) struct Interpreted {
    pub answers: Vec<Message>,
    pub shell_effects: Vec<ShellEffect>,
    pub flow: ControlFlow<()>,
    pub restart: Option<(Driver, Vec<Effect>)>,
}

impl Default for Interpreted {
    fn default() -> Self {
        Self {
            answers: Vec::new(),
            shell_effects: Vec::new(),
            flow: ControlFlow::Continue(()),
            restart: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Interpreter<'a> {
    pub drivers: &'a Drivers,
    pub ports: &'a Ports,
    pub timers: &'a mut Timers,
    pub trace: &'a mut Trace,
}

impl Interpreter<'_> {
    fn trace_undelivered(&mut self, result: Result<(), crate::port::Undelivered>) {
        if let Err(undelivered) = result {
            self.trace.push(undelivered.into());
        }
    }

    fn send_audio(&mut self, command: AudioCmd) {
        let result = self.ports.audio.send(self.drivers, command);
        self.trace_undelivered(result);
    }

    fn send_library(&mut self, command: LibraryCmd) {
        let result = self.ports.library.send_command(self.drivers, command);
        self.trace_undelivered(result);
    }

    fn send_macos(&mut self, command: MacosCmd) {
        let result = self.ports.macos.send(self.drivers, command);
        self.trace_undelivered(result);
    }

    fn config(&mut self, command: ConfigCmd) {
        let result = self.ports.config.send(self.drivers, command);
        self.trace_undelivered(result);
    }
}

fn shuffle_order(len: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    fastrand::shuffle(&mut order);
    order
}

pub(crate) fn interpret(cmd: Cmd, interpreter: &mut Interpreter<'_>) -> Interpreted {
    let mut interpreted = Interpreted::default();
    let mut effects = cmd.into_iter();
    while let Some(effect) = effects.next() {
        match effect {
            Effect::Audio(command) => interpreter.send_audio(command),
            Effect::Library(command) => interpreter.send_library(command),
            Effect::Macos(command) => interpreter.send_macos(command),
            Effect::Config(command) => interpreter.config(command),
            Effect::WindowColors(command) => {
                interpreted
                    .shell_effects
                    .push(ShellEffect::WindowColors(command));
            }
            Effect::Animate(cue) => {
                interpreted.shell_effects.push(ShellEffect::Animate(cue));
            }
            Effect::RollShuffle { len } => {
                interpreted.answers.push(Message::Loaded(
                    PlaylistRequest::ShuffleRolled(shuffle_order(len)),
                ));
            }
            Effect::After { delay, message } => {
                if let Some(deadline) = Instant::now().checked_add(delay) {
                    interpreter.timers.schedule(deadline, message);
                } else {
                    let timer: &'static str = (&message).into();
                    interpreter.trace.push(TraceEntry::TimerOverflow { timer });
                }
            }
            Effect::Restart(driver) => {
                interpreted.restart = Some((driver, effects.collect()));
                return interpreted;
            }
            Effect::Quit => interpreted.flow = ControlFlow::Break(()),
        }
    }
    interpreted
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::{Receiver, never, unbounded};
    use kernel::{
        AudioCmd,
        Cmd,
        ConfigCmd,
        ConfigPatch,
        Cue,
        Effect,
        LibraryCmd,
        MacosCmd,
        Message,
        PlaylistRequest,
        Timer,
        WindowColorsCmd,
        domain::{
            Driver,
            DriverStatus,
            Model,
            OptionCount,
            OutputDevice,
            Revision,
            SettingId,
        },
    };

    use crate::{
        interpret::{Interpreter, interpret},
        library::machine::LibraryMessage,
        port::{LibraryPort, Port, Ports},
        sender::FullEdge,
        shell::ShellEffect,
        timers::Timers,
        trace::{DropReason, Trace, TraceEntry},
    };

    struct Fixture {
        model: Model,
        ports: Ports,
        audio_rx: Receiver<AudioCmd>,
        library_rx: Receiver<LibraryMessage>,
        config_rx: Receiver<ConfigCmd>,
        macos_rx: Receiver<MacosCmd>,
        timers: Timers,
        trace: Trace,
    }

    impl Fixture {
        fn new() -> Self {
            let (audio_tx, audio_rx) = unbounded();
            let (library_tx, library_rx) = unbounded();
            let (config_tx, config_rx) = unbounded();
            let (macos_tx, macos_rx) = unbounded();
            let macos_port = Port::new(Driver::Macos, macos_tx, FullEdge::default());
            Self {
                model: Model::default(),
                ports: Ports {
                    audio: Port::new(Driver::Audio, audio_tx, FullEdge::default()),
                    library: LibraryPort::new(Port::new(
                        Driver::Library,
                        library_tx,
                        FullEdge::default(),
                    )),
                    config: Port::new(Driver::Config, config_tx, FullEdge::default()),
                    macos: macos_port,
                },
                audio_rx,
                library_rx,
                config_rx,
                macos_rx,
                timers: Timers::default(),
                trace: Trace::default(),
            }
        }

        fn interpreter(&mut self) -> Interpreter<'_> {
            Interpreter {
                drivers: &self.model.drivers,
                ports: &self.ports,
                timers: &mut self.timers,
                trace: &mut self.trace,
            }
        }
    }

    #[test]
    fn a_command_to_a_running_driver_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        interpret(Cmd::One(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
        assert!(fixture.trace.is_empty());
    }

    #[test]
    fn a_restart_effect_hands_back_the_driver_and_the_rest() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();
        let cmd = Cmd::Batch(vec![
            Effect::Restart(Driver::Audio),
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        ]);

        let interpreted = interpret(cmd, &mut interpreter);

        assert_eq!(
            interpreted.restart,
            Some((
                Driver::Audio,
                vec![
                    Effect::Audio(AudioCmd::Stop),
                    Effect::Audio(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
                ]
            ))
        );
        assert!(fixture.audio_rx.try_recv().is_err());
    }

    #[test]
    fn a_command_to_a_dead_driver_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.model.drivers.record_mut(Driver::Audio).status = DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter();

        interpret(Cmd::One(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert!(fixture.audio_rx.try_recv().is_err());
        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Audio,
                command: "stop",
                reason: DropReason::NotRunning,
            })
        );
    }

    #[test]
    fn a_send_onto_a_lost_inbox_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.audio_rx = never();
        let mut interpreter = fixture.interpreter();

        interpret(Cmd::One(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Audio,
                command: "stop",
                reason: DropReason::Closed,
            })
        );
    }

    #[test]
    fn a_system_command_to_a_stopped_macos_driver_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.model.drivers.record_mut(Driver::Macos).status = DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter();

        interpret(
            Cmd::One(Effect::Macos(MacosCmd::Volume(kernel::Percent::default()))),
            &mut interpreter,
        );

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Macos,
                command: "volume",
                reason: DropReason::NotRunning,
            })
        );
    }

    #[test]
    fn a_system_send_onto_a_lost_macos_inbox_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.macos_rx = never();
        let mut interpreter = fixture.interpreter();

        interpret(
            Cmd::One(Effect::Macos(MacosCmd::Volume(kernel::Percent::default()))),
            &mut interpreter,
        );

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: Driver::Macos,
                command: "volume",
                reason: DropReason::Closed,
            })
        );
    }

    #[test]
    fn a_library_command_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        interpret(
            Cmd::One(Effect::Library(LibraryCmd::LoadFavorites)),
            &mut interpreter,
        );

        assert!(matches!(
            fixture.library_rx.try_recv(),
            Ok(LibraryMessage::Cmd(LibraryCmd::LoadFavorites))
        ));
    }

    #[test]
    fn a_config_save_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        interpret(
            Cmd::One(Effect::Config(ConfigCmd::Save(
                ConfigPatch::builder()
                    .theme(kernel::domain::ThemeName::from_static("dark"))
                    .build(),
            ))),
            &mut interpreter,
        );

        assert!(matches!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCmd::Save(patch))
                if patch.theme.as_ref().map(kernel::domain::ThemeName::as_str) == Some("dark")
        ));
    }

    #[test]
    fn window_colors_and_animate_become_shell_effects() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        let interpreted = interpret(
            Cmd::Batch(vec![
                Effect::WindowColors(WindowColorsCmd::Reset),
                Effect::Animate(Cue::TrackChanged),
            ]),
            &mut interpreter,
        );

        assert_eq!(
            interpreted.shell_effects,
            vec![
                ShellEffect::WindowColors(WindowColorsCmd::Reset),
                ShellEffect::Animate(Cue::TrackChanged),
            ]
        );
    }

    #[test]
    fn quit_stops_the_flow_after_the_rest_of_the_batch() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        let interpreted = interpret(
            Cmd::Batch(vec![Effect::Audio(AudioCmd::Stop), Effect::Quit]),
            &mut interpreter,
        );

        assert_eq!(interpreted.flow, std::ops::ControlFlow::Break(()));
        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
    }

    #[test]
    fn roll_shuffle_answers_with_a_permutation_of_the_right_length() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        let interpreted =
            interpret(Cmd::One(Effect::RollShuffle { len: 5 }), &mut interpreter);
        let [Message::Loaded(PlaylistRequest::ShuffleRolled(order))] =
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
        let mut interpreter = fixture.interpreter();

        let interpreted = interpret(
            Cmd::Batch(vec![
                Effect::RollShuffle { len: 2 },
                Effect::RollShuffle { len: 3 },
            ]),
            &mut interpreter,
        );

        let [
            Message::Loaded(PlaylistRequest::ShuffleRolled(first)),
            Message::Loaded(PlaylistRequest::ShuffleRolled(second)),
        ] = interpreted.answers.as_slice()
        else {
            panic!("expected two shuffle answers in order");
        };
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 3);
    }

    fn setting_id(field: config::AppearanceField) -> SettingId {
        config::APPEARANCE_ROWS[field as usize].custom.id
    }

    #[test]
    fn a_setting_effect_reaches_the_config_inbox_as_a_setting_command() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();
        let id = setting_id(config::AppearanceField::CoverBrackets);
        let option = OptionCount::new(2).unwrap().index(0).unwrap();

        let interpreted = interpret(
            Cmd::One(Effect::Config(ConfigCmd::Setting { id, option })),
            &mut interpreter,
        );

        assert_eq!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCmd::Setting { id, option })
        );
        assert!(interpreted.shell_effects.is_empty());
        assert!(fixture.trace.iter().next().is_none());
    }

    #[test]
    fn after_schedules_a_timer() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

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
        let mut interpreter = fixture.interpreter();

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
