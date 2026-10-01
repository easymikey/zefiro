use kernel::{
    AudioCmd,
    AudioError,
    AudioEvent,
    Playback,
    TrackRequest,
    domain::OutputDevice,
    update::Rejected,
};

use crate::{
    EngineConfig,
    deck::DeviceOpened,
    engine::{
        effect::{EngineEffect, EngineMessage},
        machine::EngineError,
        state::{Engine, Live, Muted, announce},
    },
};

impl Muted {
    pub(crate) fn transition(
        self,
        message: EngineMessage,
    ) -> Result<(Engine, EngineEffect), Box<Rejected<Engine>>> {
        match message {
            EngineMessage::Cmd(AudioCmd::SetDevice(device)) => Ok(self.retry(device)),
            EngineMessage::Cmd(AudioCmd::Load(request)) => Ok(self.wait_for(request)),
            EngineMessage::Cmd(AudioCmd::ListDevices) => {
                Ok((Engine::Muted(self), EngineEffect::ListDevices))
            }
            EngineMessage::Cmd(AudioCmd::SetSpeed(speed)) => Ok((
                Engine::Muted(Muted { speed, ..self }),
                EngineEffect::Nothing,
            )),
            EngineMessage::Cmd(AudioCmd::SetCrossfade(crossfade)) => {
                let config = EngineConfig {
                    crossfade,
                    ..self.config
                };
                Ok((
                    Engine::Muted(Muted { config, ..self }),
                    EngineEffect::Nothing,
                ))
            }
            EngineMessage::Cmd(AudioCmd::SetReplaygain(replaygain)) => {
                let config = EngineConfig {
                    replaygain,
                    ..self.config
                };
                Ok((
                    Engine::Muted(Muted { config, ..self }),
                    EngineEffect::Nothing,
                ))
            }
            EngineMessage::Cmd(AudioCmd::Stop) => Ok(self.stop()),
            EngineMessage::Opened(Ok(reopened)) => Ok(self.reopened(reopened)),
            EngineMessage::Opened(Err(error)) => Ok(self.stays_silent(error)),
            EngineMessage::DevicesListed(devices) => Ok((
                Engine::Muted(self),
                EngineEffect::Send(AudioEvent::DevicesListed(devices)),
            )),
            EngineMessage::Cmd(AudioCmd::Playback(Playback::Paused))
            | EngineMessage::Decoded(_)
            | EngineMessage::Preloaded(_)
            | EngineMessage::Failed(_)
            | EngineMessage::Finished(_)
            | EngineMessage::Cued
            | EngineMessage::Ramped(_) => {
                Ok((Engine::Muted(self), EngineEffect::Nothing))
            }
            EngineMessage::Cmd(
                AudioCmd::Playback(Playback::Playing)
                | AudioCmd::Seek(_)
                | AudioCmd::Preload(_),
            ) => Err(Box::new(Rejected {
                reason: EngineError::WhileMuted(self.error.clone()),
                state: Engine::Muted(self),
            })),
        }
    }

    fn retry(self, device: OutputDevice) -> (Engine, EngineEffect) {
        let speed = self.speed;
        let asked = Muted {
            config: EngineConfig {
                device: device.clone(),
                ..self.config
            },
            ..self
        };
        (Engine::Muted(asked), EngineEffect::Open { device, speed })
    }

    fn wait_for(self, pending: TrackRequest) -> (Engine, EngineEffect) {
        let device = self.config.device.clone();
        let speed = self.speed;
        let waiting = Muted {
            pending: Some(pending),
            ..self
        };
        (Engine::Muted(waiting), EngineEffect::Open { device, speed })
    }

    fn stop(self) -> (Engine, EngineEffect) {
        let stopped = Muted {
            pending: None,
            ..self
        };
        (Engine::Muted(stopped), EngineEffect::Nothing)
    }

    fn stays_silent(self, error: AudioError) -> (Engine, EngineEffect) {
        let kept = Muted {
            error: error.clone(),
            pending: None,
            ..self
        };
        (
            Engine::Muted(kept),
            EngineEffect::Send(AudioEvent::Error(error)),
        )
    }

    fn reopened(self, reopened: DeviceOpened) -> (Engine, EngineEffect) {
        let DeviceOpened { device, opened, .. } = reopened;
        let live = Live::new(
            EngineConfig {
                device: device.clone(),
                ..self.config
            },
            self.speed,
        );
        let (engine, effect) = match self.pending {
            None => (Engine::Live(live), EngineEffect::Nothing),
            Some(pending) => live.load(pending),
        };
        (engine, announce(opened, device, effect))
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
        Playback,
        TrackRequest,
        domain::{DeviceName, OutputDevice, Replaygain, Speed},
        update::Machine,
    };
    use rstest::rstest;

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage},
            machine::EngineError,
            state::{Engine, Live, Muted},
            test_support::{
                Cell,
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
                track_b,
                waiting_for,
            },
        },
    };

    fn trace(
        state: Engine,
        messages: Vec<EngineMessage>,
    ) -> (Engine, Vec<EngineEffect>) {
        let mut current = state;
        let log = messages
            .into_iter()
            .map(|message| current.update(message).unwrap())
            .collect();
        (current, log)
    }

    fn silenced(engine: Engine) -> (AudioError, Option<TrackRequest>) {
        match engine {
            Engine::Muted(Muted {
                error,
                config: held,
                pending,
                ..
            }) => {
                assert_eq!(held, config());
                (error, pending)
            }
            Engine::Live(_) => panic!("the engine must be muted"),
        }
    }

    #[rstest]
    #[case::muted_retries_a_device(
        muted(),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        Cell {
            next: Engine::Muted(Muted { error: error(), config: config_on("usb"), pending: None, speed: Speed::default() }),
            effect: EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            },
        }
    )]
    #[case::muted_lists_devices(
        muted(),
        cmd(AudioCmd::ListDevices),
        Cell { next: muted(), effect: EngineEffect::ListDevices }
    )]
    #[case::muted_goes_live_once_opened(
        muted(),
        opened(
            OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()), Duration::ZERO, Playback::Playing),
        Cell { next: Engine::Live(Live { config: config_on("usb"), ..live() }), effect: EngineEffect::Nothing }
    )]
    #[case::muted_adopts_the_device_that_actually_opened(
        Engine::Muted(Muted { error: error(), config: config_on("usb"), pending: None, speed: Speed::default() }),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        Cell { next: Engine::Live(live()), effect: EngineEffect::Nothing }
    )]
    #[case::muted_tells_the_world_the_device_fell_back(
        Engine::Muted(Muted { error: error(), config: config_on("usb"), pending: None, speed: Speed::default() }),
        fell_back(Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(live()),
            effect: EngineEffect::Send(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)),
        }
    )]
    #[case::a_waiting_load_starts_once_the_stream_is_back(
        waiting_for("/a"),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
        }
    )]
    #[case::a_waiting_load_starts_after_the_fallback_is_announced(
        waiting_for("/a"),
        fell_back(Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Send(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)),
                EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
            ]),
        }
    )]
    #[case::a_waiting_load_is_dropped_when_the_stream_stays_dead(
        waiting_for("/a"),
        EngineMessage::Opened(Err(output_lost())),
        Cell {
            next: Engine::Muted(Muted { error: output_lost(), config: config(), pending: None, speed: Speed::default() }),
            effect: EngineEffect::Send(AudioEvent::Error(output_lost())),
        }
    )]
    #[case::muted_keeps_the_new_error(
        muted(),
        EngineMessage::Opened(Err(decode_error())),
        Cell {
            next: Engine::Muted(Muted { error: decode_error(), config: config(), pending: None, speed: Speed::default() }),
            effect: EngineEffect::Send(AudioEvent::Error(decode_error())),
        }
    )]
    #[case::muted_remembers_the_speed(
        muted(),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        Cell {
            next: Engine::Muted(Muted {
                error: error(),
                config: config(),
                pending: None,
                speed: Speed::clamped(1.5),
            }),
            effect: EngineEffect::Nothing,
        }
    )]
    #[case::muted_remembers_the_crossfade(
        muted(),
        set_crossfade(4),
        Cell {
            next: Engine::Muted(Muted {
                error: error(),
                config: EngineConfig { crossfade: crossfade(4), ..config() },
                pending: None,
                speed: Speed::default(),
            }),
            effect: EngineEffect::Nothing,
        }
    )]
    #[case::muted_remembers_the_replaygain(
        muted(),
        cmd(AudioCmd::SetReplaygain(Replaygain::On)),
        Cell {
            next: Engine::Muted(Muted {
                error: error(),
                config: EngineConfig { replaygain: Replaygain::On, ..config() },
                pending: None,
                speed: Speed::default(),
            }),
            effect: EngineEffect::Nothing,
        }
    )]
    #[case::muted_stop_clears_a_pending_load(
        waiting_for("/a"),
        cmd(AudioCmd::Stop),
        Cell {
            next: Engine::Muted(Muted {
                error: error(),
                config: config(),
                pending: None,
                speed: Speed::default(),
            }),
            effect: EngineEffect::Nothing,
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Cell,
    ) {
        assert_cell(start, message, moved);
    }

    #[rstest]
    #[case::muted_ignores_a_pause(muted(), cmd(AudioCmd::Playback(Playback::Paused)))]
    #[case::muted_ignores_a_decode(muted(), EngineMessage::Decoded(Ok(None)))]
    #[case::muted_ignores_a_preload_answer(muted(), installed(track_b()))]
    #[case::muted_ignores_a_second_stream_error(muted(), failed())]
    fn a_stale_cell_leaves_the_muted_engine_alone(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.update(message), Ok(EngineEffect::Nothing));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::muted_refuses_a_resume(muted(), cmd(AudioCmd::Playback(Playback::Playing)))]
    #[case::muted_refuses_seek(muted(), cmd(AudioCmd::Seek(seconds(5))))]
    #[case::muted_refuses_preload(muted(), preload("/b"))]
    fn a_refused_cell_hands_the_state_back_with_the_error(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.update(message), Err(EngineError::WhileMuted(error())));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::a_stream_error_while_live(failed(), output_lost())]
    #[case::a_reopen_error_while_live(EngineMessage::Opened(Err(error())), error())]
    fn an_error_mutes_the_engine_once(
        #[case] message: EngineMessage,
        #[case] expected: AudioError,
    ) {
        let (engine, log) = trace(Engine::Live(playing()), vec![message]);
        assert_eq!(log, vec![EngineEffect::Mute(expected.clone())]);

        let (held, pending) = silenced(engine);
        assert_eq!(held, expected);
        assert_eq!(pending, None);
    }

    #[test]
    fn a_load_while_muted_reopens_and_then_plays() {
        let (engine, mut log) = trace(muted(), vec![load("/a")]);
        let (_, pending) = silenced(engine.clone());
        assert_eq!(pending.map(|pending| pending.path), Some("/a".into()));

        let (engine, tail) = trace(
            engine,
            vec![
                opened(OutputDevice::SystemDefault, seconds(0), Playback::Playing),
                EngineMessage::Decoded(Ok(Some(TOTAL))),
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
            vec![load("/a"), EngineMessage::Opened(Err(output_lost()))],
        );
        assert_eq!(
            log,
            vec![
                EngineEffect::Open {
                    device: OutputDevice::SystemDefault,
                    speed: Speed::default()
                },
                EngineEffect::Send(AudioEvent::Error(output_lost())),
            ]
        );

        let (held, pending) = silenced(engine);
        assert_eq!(held, output_lost());
        assert_eq!(pending, None);
    }
}
