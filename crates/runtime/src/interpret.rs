use std::ops::ControlFlow;

use kernel::{cmd::Effect, domain::index::ViewIndex, message::Message};

use crate::{runtime::Runtime, shell::ShellEffect};

fn shuffle_order(len: usize) -> Vec<ViewIndex> {
    let mut order: Vec<ViewIndex> = (0..len).map(ViewIndex::new).collect();
    fastrand::shuffle(&mut order);
    order
}

impl Runtime {
    pub(crate) fn interpret(&mut self, effects: Vec<Effect>) -> Vec<Message> {
        let mut answers = Vec::new();
        for effect in effects {
            match effect {
                Effect::Audio(cmd) => {
                    self.wiring.ports.audio.send(&self.model.drivers, cmd);
                }
                Effect::Library(cmd) => {
                    self.wiring.ports.library.send(&self.model.drivers, cmd);
                }
                Effect::Macos(cmd) => {
                    self.wiring.ports.macos.send(&self.model.drivers, cmd);
                }
                Effect::Remote(cmd) => {
                    self.wiring.ports.remote.send(&self.model.drivers, cmd);
                }
                Effect::Config(cmd) => {
                    self.wiring.ports.config.send(&self.model.drivers, cmd);
                }
                Effect::WindowColors(cmd) => {
                    self.shell_effects.push(ShellEffect::WindowColors(cmd));
                }
                Effect::Animate(cue) => {
                    self.shell_effects.push(ShellEffect::Animate(cue));
                }
                Effect::RollShuffle(len) => {
                    answers.push(Message::ShuffleRolled(shuffle_order(len)));
                }
                Effect::After { delay, timer } => self.timers.after(delay, timer),
                Effect::Restart(driver) => {
                    answers.extend(self.wiring.restart(driver, &self.model));
                }
                Effect::Quit => self.flow = ControlFlow::Break(()),
            }
        }
        answers
    }
}

#[cfg(test)]
mod tests {
    use std::{ops::ControlFlow, path::PathBuf, time::Duration};

    use crossbeam_channel::{Receiver, TryRecvError, never, unbounded};
    use kernel::{
        cmd::{
            AudioCmd,
            ConfigCmd,
            ConfigPatch,
            CoverJob,
            DiskCmd,
            Effect,
            LibraryCmd,
            MacosCmd,
            RemoteCmd,
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
            percent::Percent,
            revision::Revision,
            server::{Account, Connection, Credential, Endpoint, ServerName, UserName},
            theme::ThemeName,
        },
        message::{Message, Timer},
    };
    use rstest::rstest;

    use crate::{
        driver_thread::Congestion,
        port::{Port, Ports},
        runtime::Runtime,
        shell::ShellEffect,
        wiring::Wiring,
    };

    struct Fixture {
        runtime: Runtime,
        audio_receiver: Receiver<AudioCmd>,
        library_receiver: Receiver<LibraryCmd>,
        config_receiver: Receiver<ConfigCmd>,
        macos_receiver: Receiver<MacosCmd>,
        remote_receiver: Receiver<RemoteCmd>,
    }

    impl Fixture {
        fn new() -> Self {
            let (audio_tx, audio_receiver) = unbounded();
            let (library_tx, library_receiver) = unbounded();
            let (config_tx, config_receiver) = unbounded();
            let (macos_tx, macos_receiver) = unbounded();
            let (remote_tx, remote_receiver) = unbounded();
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
                remote: Port::new(DriverName::Remote, remote_tx, Congestion::default()),
            };
            Self {
                runtime: Runtime::assemble(Model::default(), Vec::new(), wiring)
                    .unwrap(),
                audio_receiver,
                library_receiver,
                config_receiver,
                macos_receiver,
                remote_receiver,
            }
        }

        fn arrived(&self) -> Vec<Effect> {
            self.audio_receiver
                .try_iter()
                .map(Effect::Audio)
                .chain(self.library_receiver.try_iter().map(Effect::Library))
                .chain(self.macos_receiver.try_iter().map(Effect::Macos))
                .chain(self.remote_receiver.try_iter().map(Effect::Remote))
                .chain(self.config_receiver.try_iter().map(Effect::Config))
                .collect()
        }
    }

    fn remote_connect() -> RemoteCmd {
        RemoteCmd::Connect(Connection {
            account: Account {
                server_name: ServerName::new("home"),
                endpoint: Endpoint::parse("https://music.example.com").unwrap(),
                user_name: UserName::new("ann").unwrap(),
            },
            credential: Credential::Stored,
        })
    }

    fn cover_job() -> CoverJob {
        CoverJob {
            path: PathBuf::from("/music/cover.jpg"),
            side: Pixels(64),
        }
    }

    fn dark_theme_save() -> ConfigCmd {
        ConfigCmd::Save(ConfigPatch {
            theme_name: Some(ThemeName::from_static("dark")),
            ..ConfigPatch::default()
        })
    }

    fn hidden_brackets() -> ConfigCmd {
        ConfigCmd::SetAppearance(AppearancePatch {
            cover_brackets: Some(CoverBrackets::Hidden),
            ..AppearancePatch::default()
        })
    }

    #[rstest]
    #[case::audio(vec![Effect::Audio(AudioCmd::Stop)])]
    #[case::macos(vec![Effect::Macos(MacosCmd::SetVolume(Percent::default()))])]
    #[case::remote(vec![Effect::Remote(remote_connect())])]
    #[case::library(vec![Effect::Library(LibraryCmd::Disk(DiskCmd::LoadFavorites))])]
    #[case::every_cover_job(vec![
        Effect::Library(LibraryCmd::DecodeCover(cover_job())),
        Effect::Library(LibraryCmd::PrefetchCover(cover_job())),
    ])]
    #[case::config_save(vec![Effect::Config(dark_theme_save())])]
    #[case::setting(vec![Effect::Config(hidden_brackets())])]
    fn a_cmd_to_a_running_driver_is_routed(#[case] effects: Vec<Effect>) {
        let mut fixture = Fixture::new();

        let answers = fixture.runtime.interpret(effects.clone());

        assert_eq!(answers, Vec::new());
        assert_eq!(fixture.arrived(), effects);
        assert_eq!(fixture.runtime.take_shell_effects(), Vec::new());
    }

    #[rstest]
    #[case::audio(DriverName::Audio, Effect::Audio(AudioCmd::Stop))]
    #[case::macos(
        DriverName::Macos,
        Effect::Macos(MacosCmd::SetVolume(Percent::default()))
    )]
    #[case::cover_job(
        DriverName::Library,
        Effect::Library(LibraryCmd::DecodeCover(cover_job()))
    )]
    fn a_cmd_to_a_dead_driver_is_dropped(
        #[case] driver_name: DriverName,
        #[case] effect: Effect,
    ) {
        let mut fixture = Fixture::new();
        fixture.runtime.model.drivers.record_mut(driver_name).status =
            DriverStatus::Stopped;

        fixture.runtime.interpret(vec![effect]);

        assert_eq!(fixture.arrived(), Vec::new());
    }

    #[rstest]
    #[case::audio(Effect::Audio(AudioCmd::Stop))]
    #[case::macos(Effect::Macos(MacosCmd::SetVolume(Percent::default())))]
    fn a_send_onto_a_lost_cmd_receiver_is_dropped(#[case] effect: Effect) {
        let mut fixture = Fixture::new();
        fixture.audio_receiver = never();
        fixture.macos_receiver = never();

        let answers = fixture.runtime.interpret(vec![effect]);

        assert_eq!(answers, Vec::new());
        assert_eq!(fixture.runtime.take_shell_effects(), Vec::new());
        assert_eq!(fixture.runtime.flow, ControlFlow::Continue(()));
    }

    #[test]
    fn a_restart_effect_replaces_the_port_before_the_rest_is_sent() {
        let mut fixture = Fixture::new();

        let answers = fixture.runtime.interpret(vec![
            Effect::Restart(DriverName::Audio),
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        ]);

        assert_eq!(answers, Vec::new());
        assert_eq!(
            fixture.audio_receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        );
    }

    #[test]
    fn window_colors_and_animate_become_shell_effects() {
        let mut fixture = Fixture::new();

        fixture.runtime.interpret(vec![
            Effect::WindowColors(WindowColorsCmd::Reset),
            Effect::Animate(Cue::TrackChanged),
        ]);

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

        fixture
            .runtime
            .interpret(vec![Effect::Audio(AudioCmd::Stop), Effect::Quit]);

        assert_eq!(fixture.runtime.flow, ControlFlow::Break(()));
        assert_eq!(fixture.audio_receiver.try_recv(), Ok(AudioCmd::Stop));
    }

    #[rstest]
    #[case::one(vec![5])]
    #[case::two_in_order(vec![2, 3])]
    fn roll_shuffle_answers_with_a_permutation_of_the_right_length(
        #[case] lens: Vec<usize>,
    ) {
        let mut fixture = Fixture::new();

        let answers = fixture
            .runtime
            .interpret(lens.iter().copied().map(Effect::RollShuffle).collect());

        let view_indexes: Vec<Vec<ViewIndex>> = answers
            .into_iter()
            .map(|answer| {
                let Message::ShuffleRolled(mut order) = answer else {
                    panic!("expected a shuffle answer");
                };
                order.sort_unstable();
                order
            })
            .collect();
        let expected: Vec<Vec<ViewIndex>> = lens
            .iter()
            .map(|&len| (0..len).map(ViewIndex::new).collect())
            .collect();
        assert_eq!(view_indexes, expected);
    }

    #[test]
    fn after_schedules_a_timer() {
        let mut fixture = Fixture::new();

        fixture.runtime.interpret(vec![Effect::After {
            delay: Duration::from_secs(1),
            timer: Timer::Toast(Revision::default()),
        }]);

        assert!(fixture.runtime.timers.next_deadline().is_some());
    }
}
