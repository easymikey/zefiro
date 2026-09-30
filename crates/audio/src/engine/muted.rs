use kernel::{
    AudioCmd,
    AudioError,
    AudioEvent,
    EngineError,
    Playback,
    domain::{Crossfade, OutputDevice, Replaygain, Speed},
    update::Rejected,
};

use crate::{
    EngineConfig,
    deck::DeviceOpened,
    engine::{
        effect::{EngineEffect, EngineMessage, devices_event},
        state::{Engine, Live, Mix, Muted, TrackRequest, Transition, announce},
    },
};

impl Muted {
    pub(crate) fn transition(self, message: EngineMessage) -> Transition {
        match message {
            EngineMessage::Cmd(AudioCmd::SetDevice(device)) => {
                Transition::from(self.retry(device))
            }
            EngineMessage::Cmd(AudioCmd::Load {
                path,
                gain,
                revision,
            }) => Transition::from(self.wait_for(TrackRequest {
                path,
                gain,
                revision,
            })),
            EngineMessage::Cmd(AudioCmd::ListDevices) => {
                Transition::Next(Engine::Muted(self), EngineEffect::ListDevices)
            }
            EngineMessage::Cmd(AudioCmd::SetSpeed(speed)) => {
                Transition::from(self.set_speed(speed))
            }
            EngineMessage::Cmd(AudioCmd::SetCrossfade(crossfade)) => {
                Transition::from(self.set_crossfade(crossfade))
            }
            EngineMessage::Cmd(AudioCmd::SetReplaygain(replaygain)) => {
                Transition::from(self.set_replaygain(replaygain))
            }
            EngineMessage::Cmd(AudioCmd::Stop) => Transition::from(self.stop()),
            EngineMessage::Opened(Ok(reopened)) => {
                Transition::from(self.reopened(reopened))
            }
            EngineMessage::Opened(Err(error)) => {
                Transition::from(self.stays_silent(error))
            }
            EngineMessage::DevicesListed(result) => Transition::Next(
                Engine::Muted(self),
                EngineEffect::Send(devices_event(result)),
            ),
            EngineMessage::Cmd(AudioCmd::Playback(Playback::Paused))
            | EngineMessage::Decoded(_)
            | EngineMessage::Preloaded(_)
            | EngineMessage::Failed(_)
            | EngineMessage::Retiring { .. }
            | EngineMessage::Finished(_)
            | EngineMessage::Cued
            | EngineMessage::Ramped(_) => {
                Transition::Next(Engine::Muted(self), EngineEffect::Nothing)
            }
            EngineMessage::Cmd(
                AudioCmd::Playback(Playback::Playing)
                | AudioCmd::Seek(_)
                | AudioCmd::Preload { .. },
            ) => Transition::Rejected(Rejected {
                reason: EngineError::WhileMuted(self.error.clone()),
                state: Engine::Muted(self),
            }),
        }
    }

    fn retry(self, device: OutputDevice) -> (Engine, EngineEffect) {
        let speed = self.mix.speed;
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
        let speed = self.mix.speed;
        let waiting = Muted {
            pending: Some(pending),
            ..self
        };
        (Engine::Muted(waiting), EngineEffect::Open { device, speed })
    }

    fn set_speed(self, speed: Speed) -> (Engine, EngineEffect) {
        let mix = Mix { speed, ..self.mix };
        (Engine::Muted(Muted { mix, ..self }), EngineEffect::Nothing)
    }

    fn set_crossfade(self, crossfade: Crossfade) -> (Engine, EngineEffect) {
        let config = EngineConfig {
            crossfade,
            ..self.config
        };
        (
            Engine::Muted(Muted { config, ..self }),
            EngineEffect::Nothing,
        )
    }

    fn set_replaygain(self, replaygain: Replaygain) -> (Engine, EngineEffect) {
        let config = EngineConfig {
            replaygain,
            ..self.config
        };
        (
            Engine::Muted(Muted { config, ..self }),
            EngineEffect::Nothing,
        )
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
        let live = Live::with_mix(
            EngineConfig {
                device: device.clone(),
                ..self.config
            },
            self.mix,
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
        EngineError,
        Playback,
        domain::{DeviceName, OutputDevice, Replaygain, Speed},
        update::Machine,
    };
    use rstest::rstest;

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage},
            state::{
                Engine,
                Live,
                Mix,
                Muted,
                TrackRequest,
                fixtures::{
                    TOTAL,
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
                    load,
                    loaded_at,
                    loading,
                    muted,
                    opened,
                    output_lost,
                    playing,
                    preload,
                    preload_b,
                    seconds,
                    set_crossfade,
                    waiting_for,
                },
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

    struct Cell {
        next: Engine,
        effect: EngineEffect,
    }

    #[rstest]
    #[case::muted_retries_a_device(
        muted(),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        Cell {
            next: Engine::Muted(Muted { error: error(), config: config_on("usb"), pending: None, mix: Mix::default() }),
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
        Cell { next: Engine::Live(Live::new(config_on("usb"))), effect: EngineEffect::Nothing }
    )]
    #[case::muted_adopts_the_device_that_actually_opened(
        Engine::Muted(Muted { error: error(), config: config_on("usb"), pending: None, mix: Mix::default() }),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        Cell { next: Engine::Live(Live::new(config())), effect: EngineEffect::Nothing }
    )]
    #[case::muted_tells_the_world_the_device_fell_back(
        Engine::Muted(Muted { error: error(), config: config_on("usb"), pending: None, mix: Mix::default() }),
        fell_back(Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(Live::new(config())),
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
            next: Engine::Muted(Muted { error: output_lost(), config: config(), pending: None, mix: Mix::default() }),
            effect: EngineEffect::Send(AudioEvent::Error(output_lost())),
        }
    )]
    #[case::muted_keeps_the_new_fault(
        muted(),
        EngineMessage::Opened(Err(decode_error())),
        Cell {
            next: Engine::Muted(Muted { error: decode_error(), config: config(), pending: None, mix: Mix::default() }),
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
                mix: Mix { speed: Speed::clamped(1.5), ..Mix::default() },
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
                mix: Mix::default(),
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
                mix: Mix::default(),
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
                mix: Mix::default(),
            }),
            effect: EngineEffect::Nothing,
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Cell,
    ) {
        let mut state = start;
        let effect = state.update(message).unwrap();
        assert_eq!(state, moved.next);
        assert_eq!(effect, moved.effect);
    }

    #[rstest]
    #[case::muted_ignores_a_pause(muted(), cmd(AudioCmd::Playback(Playback::Paused)))]
    #[case::muted_ignores_a_decode(muted(), EngineMessage::Decoded(Ok(None)))]
    #[case::muted_ignores_a_preload_answer(muted(), installed(preload_b()))]
    #[case::muted_ignores_a_retiring_fact(muted(), EngineMessage::Retiring { from: 0.0 })]
    #[case::muted_ignores_a_second_stream_fault(muted(), failed())]
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
    fn a_refused_cell_hands_the_state_back_with_the_fault(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.update(message), Err(EngineError::WhileMuted(error())));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::a_stream_fault_while_live(failed(), output_lost())]
    #[case::a_failed_reopen_while_live(EngineMessage::Opened(Err(error())), error())]
    fn a_fault_mutes_the_engine_once(
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
    fn a_muted_engine_ignores_a_retiring_fact() {
        let (mut engine, _) = trace(Engine::Live(playing()), vec![failed()]);
        let retiring = engine.update(EngineMessage::Retiring { from: 0.5 });
        assert_eq!(retiring, Ok(EngineEffect::Nothing));
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
