use std::{ops::ControlFlow, time::Instant};

use kernel::{cmd::Effect, domain::index::ViewIndex, message::Message};

use crate::{runtime::Runtime, shell::ShellEffect};

fn shuffle_order(len: usize) -> Vec<ViewIndex> {
    let mut order: Vec<ViewIndex> = (0..len).map(ViewIndex::new).collect();
    fastrand::shuffle(&mut order);
    order
}

impl Runtime {
    pub(crate) fn interpret(&mut self, effects: Vec<Effect>) -> Vec<Message> {
        effects
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Audio(command) => {
                    match self.wiring.ports.audio.send(&self.model.drivers, command) {
                        Ok(()) | Err(_) => None,
                    }
                }
                Effect::Library(command) => {
                    match self.wiring.ports.library.send(&self.model.drivers, command) {
                        Ok(()) | Err(_) => None,
                    }
                }
                Effect::Macos(command) => {
                    match self.wiring.ports.macos.send(&self.model.drivers, command) {
                        Ok(()) | Err(_) => None,
                    }
                }
                Effect::Config(command) => {
                    match self.wiring.ports.config.send(&self.model.drivers, command) {
                        Ok(()) | Err(_) => None,
                    }
                }
                Effect::WindowColors(command) => {
                    self.shell_effects.push(ShellEffect::WindowColors(command));
                    None
                }
                Effect::Animate(cue) => {
                    self.shell_effects.push(ShellEffect::Animate(cue));
                    None
                }
                Effect::RollShuffle(len) => {
                    Some(Message::ShuffleRolled(shuffle_order(len)))
                }
                Effect::After { delay, timer } => {
                    if let Some(deadline) = Instant::now().checked_add(delay) {
                        self.timers.schedule(deadline, timer);
                    }
                    None
                }
                Effect::Restart(driver) => {
                    self.wiring.restart(driver, &self.model);
                    None
                }
                Effect::Quit => {
                    self.flow = ControlFlow::Break(());
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::{ops::ControlFlow, path::PathBuf, time::Duration};

    use crossbeam_channel::{Receiver, TryRecvError, never, unbounded};
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
            appearance::{AppearancePatch, CoverBrackets},
            cue::Cue,
            device::OutputDevice,
            driver::{DriverName, DriverStatus},
            geometry::Pixels,
            index::ViewIndex,
            model::Model,
            revision::Revision,
        },
        message::{Message, Timer},
    };

    use crate::{
        driver_thread::Congestion,
        port::{Port, Ports},
        runtime::Runtime,
        shell::ShellEffect,
        wiring::Wiring,
    };

    struct Fixture {
        runtime: Runtime,
        audio_rx: Receiver<AudioCmd>,
        library_rx: Receiver<LibraryCmd>,
        config_rx: Receiver<ConfigCmd>,
        macos_rx: Receiver<MacosCmd>,
    }

    impl Fixture {
        fn new() -> Self {
            let (audio_tx, audio_rx) = unbounded();
            let (library_tx, library_rx) = unbounded();
            let (config_tx, config_rx) = unbounded();
            let (macos_tx, macos_rx) = unbounded();
            let (mut wiring, ..) = Wiring::idle();
            wiring.ports = Ports {
                audio: Port::new(DriverName::Audio, audio_tx, Congestion::default()),
                library: Port::new(
                    DriverName::Library,
                    library_tx,
                    Congestion::default(),
                ),
                config: Port::new(DriverName::Config, config_tx, Congestion::default()),
                macos: Port::new(DriverName::Macos, macos_tx, Congestion::default()),
            };
            Self {
                runtime: Runtime::assemble((Model::default(), Vec::new()), wiring)
                    .unwrap(),
                audio_rx,
                library_rx,
                config_rx,
                macos_rx,
            }
        }
    }

    fn run(cmd: Cmd, runtime: &mut Runtime) -> Vec<Message> {
        runtime.interpret(cmd.into_parts().0)
    }

    #[test]
    fn a_command_to_a_running_driver_is_routed() {
        let mut fixture = Fixture::new();

        run(
            Cmd::effect(Effect::Audio(AudioCmd::Stop)),
            &mut fixture.runtime,
        );

        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
    }

    #[test]
    fn a_restart_effect_replaces_the_port_before_the_rest_is_sent() {
        let mut fixture = Fixture::new();
        let cmd = Cmd::from_iter([
            Effect::Restart(DriverName::Audio),
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        ]);

        let answers = run(cmd, &mut fixture.runtime);

        assert_eq!(answers, Vec::new());
        assert_eq!(fixture.audio_rx.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn a_command_to_a_dead_driver_is_dropped() {
        let mut fixture = Fixture::new();
        fixture
            .runtime
            .model
            .drivers
            .record_mut(DriverName::Audio)
            .status = DriverStatus::Stopped;

        run(
            Cmd::effect(Effect::Audio(AudioCmd::Stop)),
            &mut fixture.runtime,
        );

        assert!(fixture.audio_rx.try_recv().is_err());
    }

    #[test]
    fn a_send_onto_a_lost_inbox_is_dropped() {
        let mut fixture = Fixture::new();
        fixture.audio_rx = never();

        let answers = run(
            Cmd::effect(Effect::Audio(AudioCmd::Stop)),
            &mut fixture.runtime,
        );

        assert_eq!(answers, Vec::new());
        assert_eq!(fixture.runtime.take_shell_effects(), Vec::new());
        assert_eq!(fixture.runtime.flow(), ControlFlow::Continue(()));
    }

    #[test]
    fn a_macos_command_to_a_stopped_macos_driver_is_dropped() {
        let mut fixture = Fixture::new();
        fixture
            .runtime
            .model
            .drivers
            .record_mut(DriverName::Macos)
            .status = DriverStatus::Stopped;

        run(
            Cmd::effect(Effect::Macos(MacosCmd::SetVolume(
                kernel::domain::percent::Percent::default(),
            ))),
            &mut fixture.runtime,
        );

        assert!(fixture.macos_rx.try_recv().is_err());
    }

    #[test]
    fn a_macos_send_onto_a_lost_macos_inbox_is_dropped() {
        let mut fixture = Fixture::new();
        fixture.macos_rx = never();

        let answers = run(
            Cmd::effect(Effect::Macos(MacosCmd::SetVolume(
                kernel::domain::percent::Percent::default(),
            ))),
            &mut fixture.runtime,
        );

        assert_eq!(answers, Vec::new());
        assert_eq!(fixture.runtime.take_shell_effects(), Vec::new());
        assert_eq!(fixture.runtime.flow(), ControlFlow::Continue(()));
    }

    #[test]
    fn a_library_command_is_routed() {
        let mut fixture = Fixture::new();

        run(
            Cmd::effect(Effect::Library(LibraryCmd::LoadFavorites)),
            &mut fixture.runtime,
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
        let cmd = Cmd::from_iter([
            Effect::Library(LibraryCmd::DecodeCover(cover_job())),
            Effect::Library(LibraryCmd::PrefetchCover(cover_job())),
        ]);

        run(cmd, &mut fixture.runtime);

        assert_eq!(
            fixture.library_rx.try_iter().collect::<Vec<_>>(),
            [
                LibraryCmd::DecodeCover(cover_job()),
                LibraryCmd::PrefetchCover(cover_job()),
            ]
        );
    }

    #[test]
    fn a_cover_job_for_a_stopped_library_is_dropped() {
        let mut fixture = Fixture::new();
        fixture
            .runtime
            .model
            .drivers
            .record_mut(DriverName::Library)
            .status = DriverStatus::Stopped;

        run(
            Cmd::effect(Effect::Library(LibraryCmd::DecodeCover(cover_job()))),
            &mut fixture.runtime,
        );

        assert!(fixture.library_rx.try_recv().is_err());
    }

    #[test]
    fn a_config_save_is_routed() {
        let mut fixture = Fixture::new();

        run(
            Cmd::effect(Effect::Config(ConfigCmd::Save(
                ConfigPatch::builder()
                    .theme(kernel::domain::theme::ThemeName::from_static("dark"))
                    .build(),
            ))),
            &mut fixture.runtime,
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

        run(
            Cmd::from_iter([
                Effect::WindowColors(WindowColorsCmd::Reset),
                Effect::Animate(Cue::TrackChanged),
            ]),
            &mut fixture.runtime,
        );

        assert_eq!(
            fixture.runtime.take_shell_effects(),
            vec![
                ShellEffect::WindowColors(WindowColorsCmd::Reset),
                ShellEffect::Animate(Cue::TrackChanged),
            ]
        );
    }

    #[test]
    fn quit_stops_the_flow_after_the_rest_of_the_batch() {
        let mut fixture = Fixture::new();

        run(
            Cmd::from_iter([Effect::Audio(AudioCmd::Stop), Effect::Quit]),
            &mut fixture.runtime,
        );

        assert_eq!(fixture.runtime.flow(), ControlFlow::Break(()));
        assert_eq!(fixture.audio_rx.try_recv(), Ok(AudioCmd::Stop));
    }

    #[test]
    fn roll_shuffle_answers_with_a_permutation_of_the_right_length() {
        let mut fixture = Fixture::new();

        let answers = run(Cmd::effect(Effect::RollShuffle(5)), &mut fixture.runtime);
        let [Message::ShuffleRolled(order)] = answers.as_slice() else {
            panic!("expected a single shuffle answer");
        };
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..5).map(ViewIndex::new).collect::<Vec<_>>());
    }

    #[test]
    fn two_roll_shuffles_in_one_batch_answer_in_order() {
        let mut fixture = Fixture::new();

        let answers = run(
            Cmd::from_iter([Effect::RollShuffle(2), Effect::RollShuffle(3)]),
            &mut fixture.runtime,
        );

        let [
            Message::ShuffleRolled(first),
            Message::ShuffleRolled(second),
        ] = answers.as_slice()
        else {
            panic!("expected two shuffle answers in order");
        };
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 3);
    }

    #[test]
    fn a_setting_effect_reaches_the_config_inbox_as_a_setting_command() {
        let mut fixture = Fixture::new();
        let patch = AppearancePatch::builder()
            .cover_brackets(CoverBrackets::Hidden)
            .build();

        run(
            Cmd::effect(Effect::Config(ConfigCmd::SetAppearance(patch))),
            &mut fixture.runtime,
        );

        assert_eq!(
            fixture.config_rx.try_recv(),
            Ok(ConfigCmd::SetAppearance(patch))
        );
        assert_eq!(fixture.runtime.take_shell_effects(), Vec::new());
    }

    #[test]
    fn after_schedules_a_timer() {
        let mut fixture = Fixture::new();

        run(
            Cmd::effect(Effect::After {
                delay: Duration::from_secs(1),
                timer: Timer::Toast(Revision::default()),
            }),
            &mut fixture.runtime,
        );

        assert!(fixture.runtime.timers.next_deadline().is_some());
    }

    #[test]
    fn a_delay_that_would_overflow_the_clock_is_skipped() {
        let mut fixture = Fixture::new();

        run(
            Cmd::effect(Effect::After {
                delay: Duration::MAX,
                timer: Timer::Toast(Revision::default()),
            }),
            &mut fixture.runtime,
        );

        assert!(fixture.runtime.timers.next_deadline().is_none());
    }
}
