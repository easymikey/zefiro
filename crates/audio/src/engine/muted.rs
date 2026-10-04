use kernel::{
    AudioCmd,
    AudioError,
    AudioEvent,
    Cmd,
    Playback,
    domain::AudioSettings,
    update::Unhandled,
};

use crate::{
    deck::{AudioJob, DeviceOpened},
    engine::{
        effect::{AudioMessage, EngineEffect},
        machine::batched,
        state::{Live, Muted, announce},
    },
};

impl Muted {
    pub(crate) fn transition(
        &mut self,
        message: AudioMessage,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match message {
            AudioMessage::Cmds(batch) => batched(batch, |cmd| self.command(cmd)),
            AudioMessage::Opened(Err(error)) => Ok(self.stays_silent(error)),
            AudioMessage::Deck(_)
            | AudioMessage::Opened(Ok(_))
            | AudioMessage::Reported(_)
            | AudioMessage::Error(_)
            | AudioMessage::DevicesListed(_)
            | AudioMessage::Decoded(_)
            | AudioMessage::Preloaded(_)
            | AudioMessage::Finished(_)
            | AudioMessage::Cued
            | AudioMessage::Ramped(_)
            | AudioMessage::SignalsTaken { .. } => Err(Unhandled),
        }
    }

    fn command(
        &mut self,
        cmd: AudioCmd,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match cmd {
            AudioCmd::SetDevice(device) => {
                self.settings.device = device.clone();
                Ok(Cmd::effect(EngineEffect::Open {
                    device,
                    speed: self.speed,
                }))
            }
            AudioCmd::Load(load) => {
                self.pending = Some(load);
                Ok(Cmd::effect(EngineEffect::Open {
                    device: self.settings.device.clone(),
                    speed: self.speed,
                }))
            }
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(EngineEffect::Run(AudioJob::ListDevices)))
            }
            AudioCmd::SetSpeed(speed) => {
                self.speed = speed;
                Ok(Cmd::none())
            }
            AudioCmd::SetCrossfade(crossfade) => {
                self.settings.crossfade = crossfade;
                Ok(Cmd::none())
            }
            AudioCmd::SetReplayGain(replay_gain) => {
                self.settings.replay_gain = replay_gain;
                Ok(Cmd::none())
            }
            AudioCmd::Stop => {
                self.pending = None;
                Ok(Cmd::none())
            }
            AudioCmd::Playback(Playback::Paused) => Ok(Cmd::none()),
            AudioCmd::Playback(Playback::Playing)
            | AudioCmd::Seek(_)
            | AudioCmd::Preload(_) => Err(Unhandled),
        }
    }

    fn stays_silent(&mut self, error: AudioError) -> Cmd<EngineEffect, AudioEvent> {
        self.pending = None;
        Cmd::message(AudioEvent::Error(error))
    }

    pub(crate) fn reopened(
        &self,
        reopened: DeviceOpened,
    ) -> (Live, Cmd<EngineEffect, AudioEvent>) {
        let DeviceOpened { device, opened, .. } = reopened;
        let mut live = Live::new(
            AudioSettings {
                device: device.clone(),
                ..self.settings.clone()
            },
            self.speed,
        );
        let cmd = self
            .pending
            .clone()
            .map_or_else(Cmd::none, |pending| live.load(pending));
        (live, announce(opened, device, cmd))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        AudioCmd,
        AudioError,
        AudioEvent,
        Bounded,
        Cmd,
        Playback,
        TrackLoad,
        domain::{AudioSettings, DeviceName, OutputDevice, ReplayGain, Speed},
        update::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::engine::{
        effect::{AudioMessage, EngineEffect},
        state::{Engine, Live, Muted},
        tests::{
            EngineRow,
            TOTAL,
            assert_cell,
            cmd,
            config,
            config_on,
            crossfade,
            decode_error,
            error,
            failed,
            fell_back,
            first,
            installed,
            live,
            load,
            loaded_at,
            loading,
            muted,
            opened,
            output_lost,
            playing,
            preload,
            seconds,
            set_crossfade,
            trace,
            track_b,
            waiting_for,
        },
    };

    fn silenced(engine: Engine) -> Option<TrackLoad> {
        match engine {
            Engine::Muted(Muted {
                settings: held,
                pending,
                ..
            }) => {
                assert_eq!(held, config());
                pending
            }
            Engine::Live(_) => panic!("the engine must be muted"),
        }
    }

    #[rstest]
    #[case::muted_retries_a_device(
        muted(),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        EngineRow {
            next: Engine::Muted(Muted { settings: config_on("usb"), pending: None, speed: Speed::default() }),
            effect: Cmd::effect(EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            }),
        }
    )]
    #[case::muted_lists_devices(
        muted(),
        cmd(AudioCmd::ListDevices),
        EngineRow { next: muted(), effect: Cmd::effect(EngineEffect::Run(crate::deck::AudioJob::ListDevices))}
    )]
    #[case::muted_goes_live_once_opened(
        muted(),
        opened(
            OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()), Duration::ZERO, Playback::Playing),
        EngineRow { next: Engine::Live(Live { settings: config_on("usb"), ..live() }), effect: Cmd::none()}
    )]
    #[case::muted_adopts_the_device_that_actually_opened(
        Engine::Muted(Muted { settings: config_on("usb"), pending: None, speed: Speed::default() }),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        EngineRow { next: Engine::Live(live()), effect: Cmd::none()}
    )]
    #[case::muted_tells_the_world_the_device_fell_back(
        Engine::Muted(Muted { settings: config_on("usb"), pending: None, speed: Speed::default() }),
        fell_back(Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(live()),
            effect: Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)),
        }
    )]
    #[case::a_waiting_load_starts_once_the_stream_is_back(
        waiting_for("/a"),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: Cmd::effect(EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() }),
        }
    )]
    #[case::a_waiting_load_starts_after_the_fallback_is_announced(
        waiting_for("/a"),
        fell_back(Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)).then(Cmd::effect(EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() })),
        }
    )]
    #[case::a_waiting_load_is_dropped_when_the_stream_stays_dead(
        waiting_for("/a"),
        AudioMessage::Opened(Err(output_lost())),
        EngineRow {
            next: Engine::Muted(Muted { settings: config(), pending: None, speed: Speed::default() }),
            effect: Cmd::message(AudioEvent::Error(output_lost())),
        }
    )]
    #[case::muted_keeps_the_new_error(
        muted(),
        AudioMessage::Opened(Err(decode_error())),
        EngineRow {
            next: Engine::Muted(Muted { settings: config(), pending: None, speed: Speed::default() }),
            effect: Cmd::message(AudioEvent::Error(decode_error())),
        }
    )]
    #[case::muted_remembers_the_speed(
        muted(),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        EngineRow {
            next: Engine::Muted(Muted {
                settings: config(),
                pending: None,
                speed: Speed::clamped(1.5),
            }),
            effect: Cmd::none(),
        }
    )]
    #[case::muted_remembers_the_crossfade(
        muted(),
        set_crossfade(4),
        EngineRow {
            next: Engine::Muted(Muted {
                settings: AudioSettings { crossfade: crossfade(4), ..config() },
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::none(),
        }
    )]
    #[case::muted_remembers_the_replaygain(
        muted(),
        cmd(AudioCmd::SetReplayGain(ReplayGain::On)),
        EngineRow {
            next: Engine::Muted(Muted {
                settings: AudioSettings { replay_gain: ReplayGain::On, ..config() },
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::none(),
        }
    )]
    #[case::muted_stop_clears_a_pending_load(
        waiting_for("/a"),
        cmd(AudioCmd::Stop),
        EngineRow {
            next: Engine::Muted(Muted {
                settings: config(),
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::none(),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: AudioMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
    }

    #[test]
    fn a_pause_leaves_the_muted_engine_alone() {
        let mut state = muted();
        assert_eq!(
            state.transition(cmd(AudioCmd::Playback(Playback::Paused))),
            Ok(Cmd::none())
        );
        assert_eq!(state, muted());
    }

    #[rstest]
    #[case::muted_ignores_a_decode(muted(), AudioMessage::Decoded(Ok(None)))]
    #[case::muted_ignores_a_preload_answer(muted(), installed(track_b()))]
    #[case::muted_ignores_a_second_stream_error(muted(), failed())]
    fn a_stale_cell_leaves_the_muted_engine_alone(
        #[case] start: Engine,
        #[case] message: AudioMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.transition(message), Err(Unhandled));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::muted_refuses_a_resume(muted(), cmd(AudioCmd::Playback(Playback::Playing)))]
    #[case::muted_refuses_seek(muted(), cmd(AudioCmd::Seek(seconds(5))))]
    #[case::muted_refuses_preload(muted(), preload("/b"))]
    fn a_refused_cell_leaves_the_state_and_names_the_error(
        #[case] start: Engine,
        #[case] message: AudioMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.transition(message), Err(Unhandled));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::a_stream_error_while_live(failed(), output_lost())]
    #[case::a_reopen_error_while_live(AudioMessage::Opened(Err(error())), error())]
    fn an_error_mutes_the_engine_once(
        #[case] message: AudioMessage,
        #[case] expected: AudioError,
    ) {
        let (engine, log) = trace(Engine::Live(playing()), vec![message]);
        assert_eq!(
            log,
            vec![
                Cmd::effect(EngineEffect::Mute)
                    .then(Cmd::message(AudioEvent::Error(expected)))
            ]
        );

        assert_eq!(silenced(engine), None);
    }

    #[test]
    fn a_load_while_muted_reopens_and_then_plays() {
        let (engine, mut log) = trace(muted(), vec![load("/a")]);
        let pending = silenced(engine.clone());
        assert_eq!(pending.map(|pending| pending.path), Some("/a".into()));

        let (engine, tail) = trace(
            engine,
            vec![
                opened(OutputDevice::SystemDefault, seconds(0), Playback::Playing),
                AudioMessage::Decoded(Ok(Some(TOTAL))),
            ],
        );
        log.extend(tail);
        insta::assert_debug_snapshot!(log);
        assert!(matches!(engine, Engine::Live(_)));
    }

    #[test]
    fn a_load_while_muted_on_a_dead_device_reports_again() {
        let (engine, log) = trace(
            muted(),
            vec![load("/a"), AudioMessage::Opened(Err(output_lost()))],
        );
        assert_eq!(
            log,
            vec![
                Cmd::effect(EngineEffect::Open {
                    device: OutputDevice::SystemDefault,
                    speed: Speed::default()
                }),
                Cmd::message(AudioEvent::Error(output_lost())),
            ]
        );

        assert_eq!(silenced(engine), None);
    }
}
