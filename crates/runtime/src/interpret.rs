use std::{ops::ControlFlow, time::Instant};

use kernel::{
    cmd::Effect,
    domain::{
        driver::{DriverName, Drivers},
        index::TrackIndex,
    },
    message::Message,
};

use crate::{
    port::Ports,
    shell::ShellEffect,
    timers::Timers,
    trace::{Trace, TraceEntry},
};

#[derive(Debug, PartialEq)]
pub(crate) struct Interpreted {
    pub(crate) answers: Vec<Message>,
    pub(crate) shell_effects: Vec<ShellEffect>,
    pub(crate) flow: ControlFlow<()>,
    pub(crate) restart: Option<(DriverName, Vec<Effect>)>,
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
    pub(crate) drivers: &'a Drivers,
    pub(crate) ports: &'a Ports,
    pub(crate) timers: &'a mut Timers,
    pub(crate) trace: &'a mut Trace,
}

fn shuffle_order(len: usize) -> Vec<TrackIndex> {
    let mut order: Vec<TrackIndex> = (0..len).map(TrackIndex::new).collect();
    fastrand::shuffle(&mut order);
    order
}

pub(crate) fn interpret(
    effects: Vec<Effect>,
    interpreter: &mut Interpreter<'_>,
) -> Interpreted {
    let mut interpreted = Interpreted::default();
    let mut effects = effects.into_iter();
    while let Some(effect) = effects.next() {
        match effect {
            Effect::Audio(command) => {
                let sent = interpreter.ports.audio.send(interpreter.drivers, command);
                interpreter.trace.record(sent);
            }
            Effect::Library(command) => {
                let sent = interpreter.ports.library.send(interpreter.drivers, command);
                interpreter.trace.record(sent);
            }
            Effect::Macos(command) => {
                let sent = interpreter.ports.macos.send(interpreter.drivers, command);
                interpreter.trace.record(sent);
            }
            Effect::Config(command) => {
                let sent = interpreter.ports.config.send(interpreter.drivers, command);
                interpreter.trace.record(sent);
            }
            Effect::WindowColors(command) => {
                interpreted
                    .shell_effects
                    .push(ShellEffect::WindowColors(command));
            }
            Effect::Animate(cue) => {
                interpreted.shell_effects.push(ShellEffect::Animate(cue));
            }
            Effect::RollShuffle(len) => {
                interpreted
                    .answers
                    .push(Message::ShuffleRolled(shuffle_order(len)));
            }
            Effect::After {
                delay,
                timer: message,
            } => {
                if let Some(deadline) = Instant::now().checked_add(delay) {
                    interpreter.timers.schedule(deadline, message);
                } else {
                    let timer: &'static str = (&message).into();
                    interpreter.trace.push(TraceEntry::TimerOverflow(timer));
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
    use std::{path::PathBuf, time::Duration};

    use crossbeam_channel::{Receiver, never, unbounded};
    use kernel::{
        cmd::{
            AudioCmd,
            Cmd,
            ConfigCmd,
            ConfigPatch,
            CoverJob,
            Effect,
            LibraryCmd,
            MacosCmd,
            WindowColorsCmd,
        },
        domain::{
            cue::Cue,
            device::OutputDevice,
            driver::{DriverName, DriverStatus},
            geometry::Pixels,
            index::TrackIndex,
            model::Model,
            revision::Revision,
            setting_row::OptionCount,
        },
        message::{Message, Timer},
    };

    use crate::{
        driver_thread::Congestion,
        interpret::{Interpreted, Interpreter, interpret},
        port::{Port, Ports},
        shell::ShellEffect,
        timers::Timers,
        trace::{DropReason, Trace, TraceEntry},
    };

    struct Fixture {
        model: Model,
        ports: Ports,
        audio_rx: Receiver<AudioCmd>,
        library_rx: Receiver<LibraryCmd>,
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
            let macos_port =
                Port::new(DriverName::Macos, macos_tx, Congestion::default());
            Self {
                model: Model::default(),
                ports: Ports {
                    audio: Port::new(
                        DriverName::Audio,
                        audio_tx,
                        Congestion::default(),
                    ),
                    library: Port::new(
                        DriverName::Library,
                        library_tx,
                        Congestion::default(),
                    ),
                    config: Port::new(
                        DriverName::Config,
                        config_tx,
                        Congestion::default(),
                    ),
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

    fn run(cmd: Cmd, interpreter: &mut Interpreter<'_>) -> Interpreted {
        interpret(cmd.into_parts().0, interpreter)
    }

    #[test]
    fn a_command_to_a_running_driver_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        run(Cmd::effect(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
        assert!(fixture.trace.is_empty());
    }

    #[test]
    fn a_restart_effect_hands_back_the_driver_and_the_rest() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();
        let cmd = Cmd::from_iter([
            Effect::Restart(DriverName::Audio),
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        ]);

        let interpreted = run(cmd, &mut interpreter);

        assert_eq!(
            interpreted.restart,
            Some((
                DriverName::Audio,
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
        fixture.model.drivers.record_mut(DriverName::Audio).status =
            DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter();

        run(Cmd::effect(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert!(fixture.audio_rx.try_recv().is_err());
        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: DriverName::Audio,
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

        run(Cmd::effect(Effect::Audio(AudioCmd::Stop)), &mut interpreter);

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: DriverName::Audio,
                command: "stop",
                reason: DropReason::Closed,
            })
        );
    }

    #[test]
    fn a_macos_command_to_a_stopped_macos_driver_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.model.drivers.record_mut(DriverName::Macos).status =
            DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::Macos(MacosCmd::SetVolume(
                kernel::domain::percent::Percent::default(),
            ))),
            &mut interpreter,
        );

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: DriverName::Macos,
                command: "set_volume",
                reason: DropReason::NotRunning,
            })
        );
    }

    #[test]
    fn a_macos_send_onto_a_lost_macos_inbox_is_dropped_and_traced() {
        let mut fixture = Fixture::new();
        fixture.macos_rx = never();
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::Macos(MacosCmd::SetVolume(
                kernel::domain::percent::Percent::default(),
            ))),
            &mut interpreter,
        );

        assert_eq!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::Dropped {
                driver: DriverName::Macos,
                command: "set_volume",
                reason: DropReason::Closed,
            })
        );
    }

    #[test]
    fn a_library_command_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::Library(LibraryCmd::LoadFavorites)),
            &mut interpreter,
        );

        assert!(matches!(
            fixture.library_rx.try_recv(),
            Ok(LibraryCmd::LoadFavorites)
        ));
    }

    fn cover_job() -> CoverJob {
        CoverJob {
            path: PathBuf::from("/music/cover.jpg"),
            side: Pixels(64),
        }
    }

    #[test]
    fn every_cover_job_is_forwarded_to_the_library() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();
        let cmd = Cmd::from_iter([
            Effect::Library(LibraryCmd::DecodeCover(cover_job())),
            Effect::Library(LibraryCmd::PrefetchCover(cover_job())),
        ]);

        run(cmd, &mut interpreter);

        assert_eq!(
            fixture.library_rx.try_iter().collect::<Vec<_>>(),
            [
                LibraryCmd::DecodeCover(cover_job()),
                LibraryCmd::PrefetchCover(cover_job()),
            ]
        );
        assert!(fixture.trace.is_empty());
    }

    #[test]
    fn a_cover_job_for_a_stopped_library_is_dropped_and_traced_once() {
        let mut fixture = Fixture::new();
        fixture.model.drivers.record_mut(DriverName::Library).status =
            DriverStatus::Stopped;
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::Library(LibraryCmd::DecodeCover(cover_job()))),
            &mut interpreter,
        );

        assert!(fixture.library_rx.try_recv().is_err());
        assert_eq!(
            fixture.trace.iter().collect::<Vec<_>>(),
            [&TraceEntry::Dropped {
                driver: DriverName::Library,
                command: "decode_cover",
                reason: DropReason::NotRunning,
            }]
        );
    }

    #[test]
    fn a_config_save_is_routed() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::Config(ConfigCmd::Save(
                ConfigPatch::builder()
                    .theme(kernel::domain::theme::ThemeName::from_static("dark"))
                    .build(),
            ))),
            &mut interpreter,
        );

        assert!(matches!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCmd::Save(patch))
                if patch.theme.as_ref().map(kernel::domain::theme::ThemeName::as_str) == Some("dark")
        ));
    }

    #[test]
    fn window_colors_and_animate_become_shell_effects() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        let interpreted = run(
            Cmd::from_iter([
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

        let interpreted = run(
            Cmd::from_iter([Effect::Audio(AudioCmd::Stop), Effect::Quit]),
            &mut interpreter,
        );

        assert_eq!(interpreted.flow, std::ops::ControlFlow::Break(()));
        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
    }

    #[test]
    fn roll_shuffle_answers_with_a_permutation_of_the_right_length() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        let interpreted = run(Cmd::effect(Effect::RollShuffle(5)), &mut interpreter);
        let [Message::ShuffleRolled(order)] = interpreted.answers.as_slice() else {
            panic!("expected a single shuffle answer");
        };
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..5).map(TrackIndex::new).collect::<Vec<_>>());
    }

    #[test]
    fn two_roll_shuffles_in_one_batch_answer_in_order() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        let interpreted = run(
            Cmd::from_iter([Effect::RollShuffle(2), Effect::RollShuffle(3)]),
            &mut interpreter,
        );

        let [
            Message::ShuffleRolled(first),
            Message::ShuffleRolled(second),
        ] = interpreted.answers.as_slice()
        else {
            panic!("expected two shuffle answers in order");
        };
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 3);
    }

    #[test]
    fn a_setting_effect_reaches_the_config_inbox_as_a_setting_command() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();
        let field = kernel::domain::setting_row::AppearanceField::CoverBrackets;
        let option = OptionCount::new(2).unwrap().index(0).unwrap();

        let interpreted = run(
            Cmd::effect(Effect::Config(ConfigCmd::SetAppearance { field, option })),
            &mut interpreter,
        );

        assert_eq!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCmd::SetAppearance { field, option })
        );
        assert!(interpreted.shell_effects.is_empty());
        assert!(fixture.trace.iter().next().is_none());
    }

    #[test]
    fn after_schedules_a_timer() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::After {
                delay: Duration::from_secs(1),
                timer: Timer::Toast(Revision::default()),
            }),
            &mut interpreter,
        );

        assert!(fixture.timers.next_deadline().is_some());
    }

    #[test]
    fn a_delay_that_would_overflow_the_clock_is_traced_and_skipped() {
        let mut fixture = Fixture::new();
        let mut interpreter = fixture.interpreter();

        run(
            Cmd::effect(Effect::After {
                delay: Duration::MAX,
                timer: Timer::Toast(Revision::default()),
            }),
            &mut interpreter,
        );

        assert!(fixture.timers.next_deadline().is_none());
        assert!(matches!(
            fixture.trace.iter().next(),
            Some(&TraceEntry::TimerOverflow("toast"))
        ));
    }
}
