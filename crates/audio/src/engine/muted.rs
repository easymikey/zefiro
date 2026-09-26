use kernel::{
    AudioCmd,
    AudioEvent,
    AudioFailure,
    EngineRejection,
    Playback,
    domain::{Crossfade, DeviceName, Percent, Replaygain, Speed},
    update::Rejected,
};

use crate::{
    EngineConfig,
    deck::Reopening,
    engine::{
        effect::{EngineEffect, EngineMessage, devices_fact},
        state::{Engine, Live, Mix, Muted, PendingLoad, Transition, announce},
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
            }) => Transition::from(self.wait_for(PendingLoad {
                path,
                gain,
                revision,
            })),
            EngineMessage::Cmd(AudioCmd::ListDevices) => {
                Transition::Next(Engine::Muted(self), EngineEffect::ListDevices)
            }
            EngineMessage::Cmd(AudioCmd::Volume(volume)) => {
                Transition::from(self.set_volume(volume))
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
                EngineEffect::Send(devices_fact(result)),
            ),
            EngineMessage::Cmd(AudioCmd::Pause(Playback::Paused))
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
                AudioCmd::Pause(Playback::Playing)
                | AudioCmd::Seek(_)
                | AudioCmd::Preload { .. },
            ) => Transition::Rejected(Rejected {
                reason: EngineRejection::WhileMuted(self.fault.clone()),
                state: Engine::Muted(self),
            }),
        }
    }

    fn retry(self, device: Option<DeviceName>) -> (Engine, EngineEffect) {
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

    fn wait_for(self, pending: PendingLoad) -> (Engine, EngineEffect) {
        let device = self.config.device.clone();
        let speed = self.mix.speed;
        let waiting = Muted {
            pending: Some(pending),
            ..self
        };
        (Engine::Muted(waiting), EngineEffect::Open { device, speed })
    }

    fn set_volume(self, volume: Percent) -> (Engine, EngineEffect) {
        let mix = Mix { volume, ..self.mix };
        (Engine::Muted(Muted { mix, ..self }), EngineEffect::Nothing)
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

    fn stays_silent(self, error: AudioFailure) -> (Engine, EngineEffect) {
        let kept = Muted {
            fault: error.clone(),
            pending: None,
            ..self
        };
        (
            Engine::Muted(kept),
            EngineEffect::Send(AudioEvent::Error(error)),
        )
    }

    fn reopened(self, reopened: Reopening) -> (Engine, EngineEffect) {
        let Reopening { device, opened, .. } = reopened;
        let live = Live::with_mix(
            EngineConfig {
                device: device.clone(),
                ..self.config
            },
            self.mix,
        );
        let (engine, io) = match self.pending {
            None => (Engine::Live(live), EngineEffect::Nothing),
            Some(pending) => live.load(pending),
        };
        (engine, announce(opened, device, io))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        AudioCmd,
        AudioEvent,
        AudioFailure,
        Bounded,
        EngineRejection,
        Playback,
        domain::{DeviceName, Percent, Replaygain, Speed},
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
                PendingLoad,
                fixtures::{
                    TOTAL,
                    cmd,
                    config,
                    config_on,
                    crossfade,
                    decode_fault,
                    failed,
                    fault,
                    fell_back,
                    first,
                    landed,
                    load,
                    loaded_at,
                    loading,
                    muted,
                    opened,
                    output_lost,
                    playing,
                    preload,
                    preload_b,
                    secs,
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

    fn silenced(engine: Engine) -> (AudioFailure, Option<PendingLoad>) {
        match engine {
            Engine::Muted(Muted {
                fault,
                config: held,
                pending,
                ..
            }) => {
                assert_eq!(held, config());
                (fault, pending)
            }
            Engine::Live(_) => panic!("the engine must be muted"),
        }
    }

    struct Transition {
        next: Engine,
        io: EngineEffect,
    }

    #[rstest]
    #[case::muted_retries_a_device(
        muted(),
        cmd(AudioCmd::SetDevice(Some(DeviceName::new("usb".to_string()).unwrap()))),
        Transition {
            next: Engine::Muted(Muted { fault: fault(), config: config_on("usb"), pending: None, mix: Mix::default() }),
            io: EngineEffect::Open {
                device: Some(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            },
        }
    )]
    #[case::muted_lists_devices(
        muted(),
        cmd(AudioCmd::ListDevices),
        Transition { next: muted(), io: EngineEffect::ListDevices }
    )]
    #[case::muted_goes_live_once_opened(
        muted(),
        opened(Some("usb"), Duration::ZERO, Playback::Playing),
        Transition { next: Engine::Live(Live::new(config_on("usb"))), io: EngineEffect::Nothing }
    )]
    #[case::muted_adopts_the_device_that_actually_opened(
        Engine::Muted(Muted { fault: fault(), config: config_on("usb"), pending: None, mix: Mix::default() }),
        opened(None, Duration::ZERO, Playback::Playing),
        Transition { next: Engine::Live(Live::new(config())), io: EngineEffect::Nothing }
    )]
    #[case::muted_tells_the_world_the_device_fell_back(
        Engine::Muted(Muted { fault: fault(), config: config_on("usb"), pending: None, mix: Mix::default() }),
        fell_back(Duration::ZERO, Playback::Playing),
        Transition {
            next: Engine::Live(Live::new(config())),
            io: EngineEffect::Send(AudioEvent::DeviceFellBack(None)),
        }
    )]
    #[case::a_waiting_load_starts_once_the_stream_is_back(
        waiting_for("/a"),
        opened(None, Duration::ZERO, Playback::Playing),
        Transition {
            next: Engine::Live(loaded_at(loading(), first())),
            io: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
        }
    )]
    #[case::a_waiting_load_starts_after_the_fallback_is_announced(
        waiting_for("/a"),
        fell_back(Duration::ZERO, Playback::Playing),
        Transition {
            next: Engine::Live(loaded_at(loading(), first())),
            io: EngineEffect::Many(vec![
                EngineEffect::Send(AudioEvent::DeviceFellBack(None)),
                EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
            ]),
        }
    )]
    #[case::a_waiting_load_is_dropped_when_the_stream_stays_dead(
        waiting_for("/a"),
        EngineMessage::Opened(Err(output_lost())),
        Transition {
            next: Engine::Muted(Muted { fault: output_lost(), config: config(), pending: None, mix: Mix::default() }),
            io: EngineEffect::Send(AudioEvent::Error(output_lost())),
        }
    )]
    #[case::muted_keeps_the_new_fault(
        muted(),
        EngineMessage::Opened(Err(decode_fault())),
        Transition {
            next: Engine::Muted(Muted { fault: decode_fault(), config: config(), pending: None, mix: Mix::default() }),
            io: EngineEffect::Send(AudioEvent::Error(decode_fault())),
        }
    )]
    #[case::muted_remembers_the_volume(
        muted(),
        cmd(AudioCmd::Volume(Percent::clamped(50))),
        Transition {
            next: Engine::Muted(Muted {
                fault: fault(),
                config: config(),
                pending: None,
                mix: Mix { volume: Percent::clamped(50), ..Mix::default() },
            }),
            io: EngineEffect::Nothing,
        }
    )]
    #[case::muted_remembers_the_speed(
        muted(),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        Transition {
            next: Engine::Muted(Muted {
                fault: fault(),
                config: config(),
                pending: None,
                mix: Mix { speed: Speed::clamped(1.5), ..Mix::default() },
            }),
            io: EngineEffect::Nothing,
        }
    )]
    #[case::muted_remembers_the_crossfade(
        muted(),
        set_crossfade(4),
        Transition {
            next: Engine::Muted(Muted {
                fault: fault(),
                config: EngineConfig { crossfade: crossfade(4), ..config() },
                pending: None,
                mix: Mix::default(),
            }),
            io: EngineEffect::Nothing,
        }
    )]
    #[case::muted_remembers_the_replaygain(
        muted(),
        cmd(AudioCmd::SetReplaygain(Replaygain::On)),
        Transition {
            next: Engine::Muted(Muted {
                fault: fault(),
                config: EngineConfig { replaygain: Replaygain::On, ..config() },
                pending: None,
                mix: Mix::default(),
            }),
            io: EngineEffect::Nothing,
        }
    )]
    #[case::muted_stop_clears_a_pending_load(
        waiting_for("/a"),
        cmd(AudioCmd::Stop),
        Transition {
            next: Engine::Muted(Muted {
                fault: fault(),
                config: config(),
                pending: None,
                mix: Mix::default(),
            }),
            io: EngineEffect::Nothing,
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Transition,
    ) {
        let mut state = start;
        let effect = state.update(message).unwrap();
        assert_eq!(state, moved.next);
        assert_eq!(effect, moved.io);
    }

    #[rstest]
    #[case::muted_ignores_a_pause(muted(), cmd(AudioCmd::Pause(Playback::Paused)))]
    #[case::muted_ignores_a_decode(muted(), EngineMessage::Decoded(Ok(None)))]
    #[case::muted_ignores_a_preload_answer(muted(), landed(preload_b()))]
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
    #[case::muted_refuses_a_resume(muted(), cmd(AudioCmd::Pause(Playback::Playing)))]
    #[case::muted_refuses_seek(muted(), cmd(AudioCmd::Seek(secs(5))))]
    #[case::muted_refuses_preload(muted(), preload("/b"))]
    fn a_refused_cell_hands_the_state_back_with_the_fault(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(
            state.update(message),
            Err(EngineRejection::WhileMuted(fault()))
        );
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::a_stream_fault_while_live(failed(), output_lost())]
    #[case::a_failed_reopen_while_live(EngineMessage::Opened(Err(fault())), fault())]
    fn a_fault_mutes_the_engine_once(
        #[case] message: EngineMessage,
        #[case] expected: AudioFailure,
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
                opened(None, secs(0), Playback::Playing),
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
                    device: None,
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
