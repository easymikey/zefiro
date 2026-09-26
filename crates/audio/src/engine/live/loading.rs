use std::time::Duration;

use kernel::{AudioEvent, AudioFailure};

use crate::{
    deck::Reopening,
    engine::{
        effect::EngineEffect,
        phase::{AfterLoad, CurrentTrack, Handover, Incoming, Loading, Phase, Playing},
        state::{Engine, Live, Mix, Muted, announce, reported},
    },
};

impl Live {
    pub(crate) fn failed(self, fault: AudioFailure) -> (Engine, EngineEffect) {
        let mix = Mix {
            speed: self.speed,
            volume: self.user_factor,
        };
        (
            Engine::Muted(Muted {
                fault: fault.clone(),
                config: self.config,
                pending: None,
                mix,
            }),
            EngineEffect::Mute(fault),
        )
    }

    pub(crate) fn opened(
        self,
        outcome: Result<Reopening, AudioFailure>,
    ) -> (Engine, EngineEffect) {
        match outcome {
            Err(error) => self.failed(error),
            Ok(reopened) => {
                let announcing = reopened.opened;
                let device = reopened.device.clone();
                let (engine, io) = self.reopened(reopened);
                (engine, announce(announcing, device, io))
            }
        }
    }

    fn reopened(mut self, reopened: Reopening) -> (Engine, EngineEffect) {
        let Reopening {
            device,
            position,
            playback,
            ..
        } = reopened;
        self.config.device = device;
        let io = match std::mem::take(&mut self.phase) {
            Phase::Idle => EngineEffect::SetVolume(self.volume()),
            Phase::Loading(loading)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(loading),
                ..
            }) => {
                self.phase = Phase::Loading(loading);
                EngineEffect::SetVolume(self.volume())
            }
            Phase::Playing(Playing { current, .. })
            | Phase::Handover(Handover {
                incoming: Incoming::Playing(current),
                ..
            }) => {
                let CurrentTrack { total, gain, path } = current;
                self.phase = Phase::Loading(Loading {
                    path: path.clone(),
                    gain,
                    after_load: AfterLoad::Resume {
                        position,
                        playback,
                        total,
                    },
                });
                EngineEffect::Decode(path)
            }
        };
        (Engine::Live(self), io)
    }

    pub(crate) fn decoded(
        mut self,
        outcome: Result<Option<Duration>, AudioFailure>,
    ) -> (Engine, EngineEffect) {
        match (std::mem::take(&mut self.phase), outcome) {
            (Phase::Loading(_), Err(error)) => (
                Engine::Live(self),
                EngineEffect::Send(AudioEvent::Error(error)),
            ),
            (
                Phase::Handover(Handover {
                    incoming: Incoming::Loading(_),
                    ..
                }),
                Err(error),
            ) => (
                Engine::Live(self),
                EngineEffect::Many(vec![
                    EngineEffect::Clear,
                    EngineEffect::Send(AudioEvent::Error(error)),
                ]),
            ),
            (Phase::Loading(loading), Ok(total)) => {
                let (current, after_load) = loading.into_current(total);
                self.phase = Phase::Playing(Playing::new(current));
                self.started(&after_load)
            }
            (
                Phase::Handover(Handover {
                    outgoing,
                    incoming: Incoming::Loading(loading),
                }),
                Ok(total),
            ) => {
                let (current, after_load) = loading.into_current(total);
                self.phase = Phase::Handover(Handover {
                    outgoing,
                    incoming: Incoming::Playing(current),
                });
                self.handover_started(&after_load)
            }
            (
                phase @ (Phase::Idle
                | Phase::Playing(_)
                | Phase::Handover(Handover {
                    incoming: Incoming::Playing(_),
                    ..
                })),
                Ok(_) | Err(_),
            ) => {
                self.phase = phase;
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }

    fn started(self, after_load: &AfterLoad) -> (Engine, EngineEffect) {
        let io = match after_load {
            AfterLoad::None => EngineEffect::Start {
                volume: self.volume(),
                total: self.phase.current().and_then(|current| current.total),
            },
            AfterLoad::Resume {
                position, playback, ..
            } => EngineEffect::Resume {
                volume: self.volume(),
                position: *position,
                paused: *playback,
            },
        };
        (Engine::Live(self), reported(io))
    }

    fn handover_started(self, after_load: &AfterLoad) -> (Engine, EngineEffect) {
        let volume = self.volume();
        let length = self.config.crossfade.value();
        let start = match after_load {
            AfterLoad::None => EngineEffect::Start {
                volume,
                total: self.phase.current().and_then(|current| current.total),
            },
            AfterLoad::Resume {
                position, playback, ..
            } => EngineEffect::Resume {
                volume,
                position: *position,
                paused: *playback,
            },
        };
        let io = EngineEffect::Many(vec![
            start,
            EngineEffect::Ramp {
                length,
                playing: volume,
            },
            EngineEffect::Report,
        ]);
        (Engine::Live(self), io)
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::{AudioCmd, AudioEvent, Playback, update::Machine};
    use rstest::rstest;

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage},
            phase::{AfterLoad, Incoming, Loading, Outgoing, Phase, Playing},
            state::{
                Engine,
                Live,
                Mix,
                Muted,
                fixtures::{
                    CROSSFADE_SECONDS,
                    PRELOAD_TOTAL,
                    TOTAL,
                    cmd,
                    config,
                    config_on,
                    crossfade,
                    decode_fault,
                    fault,
                    fell_back,
                    handing_over,
                    live,
                    live_with_crossfade,
                    loading,
                    loading_track,
                    opened,
                    playing,
                    resuming,
                    retiring,
                    secs,
                    track_a,
                },
                reported,
            },
        },
    };

    struct Transition {
        next: Engine,
        io: EngineEffect,
    }

    #[rstest]
    #[case::live_adopts_the_device_that_actually_opened(
        Engine::Live(Live::new(config_on("usb"))),
        opened(None, Duration::ZERO, Playback::Playing),
        Transition { next: Engine::Live(Live::new(config())), io: EngineEffect::SetVolume(1.0) }
    )]
    #[case::a_live_engine_tells_the_world_the_device_fell_back(
        Engine::Live(Live::new(config_on("usb"))),
        fell_back(Duration::ZERO, Playback::Playing),
        Transition {
            next: Engine::Live(Live::new(config())),
            io: EngineEffect::Many(vec![
                EngineEffect::Send(AudioEvent::DeviceFellBack(None)),
                EngineEffect::SetVolume(1.0),
            ]),
        }
    )]
    #[case::open_failure_mutes_the_engine(
        Engine::Live(playing()),
        EngineMessage::Opened(Err(fault())),
        Transition {
            next: Engine::Muted(Muted {
                fault: fault(),
                config: config(),
                pending: None,
                mix: Mix::default(),
            }),
            io: EngineEffect::Mute(fault()),
        }
    )]
    #[case::reopened_resumes_the_current_track(
        Engine::Live(playing()),
        opened(Some("usb"), secs(5), Playback::Paused),
        Transition { next: Engine::Live(resuming()), io: EngineEffect::Decode("/a".into()) }
    )]
    #[case::reopened_keeps_a_pending_load(
        Engine::Live(loading()),
        opened(Some("usb"), Duration::ZERO, Playback::Playing),
        Transition {
            next: Engine::Live(Live { config: config_on("usb"), ..loading() }),
            io: EngineEffect::SetVolume(1.0),
        }
    )]
    #[case::reopened_with_nothing_loaded(
        Engine::Live(live()),
        opened(None, Duration::ZERO, Playback::Playing),
        Transition { next: Engine::Live(live()), io: EngineEffect::SetVolume(1.0) }
    )]
    #[case::decoded_starts_the_track(
        Engine::Live(loading()),
        EngineMessage::Decoded(Ok(Some(TOTAL))),
        Transition {
            next: Engine::Live(Live { phase: Phase::Playing(Playing::new(track_a())), ..playing() }),
            io: reported(EngineEffect::Start { volume: 1.0, total: Some(TOTAL) }),
        }
    )]
    #[case::decoded_resumes_where_the_old_device_was(
        Engine::Live(resuming()),
        EngineMessage::Decoded(Ok(Some(PRELOAD_TOTAL))),
        Transition {
            next: Engine::Live(Live {
                phase: Phase::Playing(Playing::new(track_a())),
                config: config_on("usb"),
                ..live()
            }),
            io: reported(
                EngineEffect::Resume { volume: 1.0, position: secs(5), paused: Playback::Paused },
            ),
        }
    )]
    #[case::decode_failure_is_reported(
        Engine::Live(loading()),
        EngineMessage::Decoded(Err(decode_fault())),
        Transition {
            next: Engine::Live(live()),
            io: EngineEffect::Send(AudioEvent::Error(decode_fault())),
        }
    )]
    #[case::reopened_mid_skip_resumes_the_incoming_track(
        Engine::Live(retiring(0.5)),
        opened(Some("usb"), secs(5), Playback::Paused),
        Transition {
            next: Engine::Live(Live {
                phase: Phase::Loading(Loading {
                    path: "/b".into(),
                    gain: None,
                    after_load: AfterLoad::Resume {
                        position: secs(5),
                        playback: Playback::Paused,
                        total: Some(PRELOAD_TOTAL),
                    },
                }),
                config: EngineConfig { crossfade: crossfade(10), ..config_on("usb") },
                ..live()
            }),
            io: EngineEffect::Decode("/b".into()),
        }
    )]
    #[case::reopened_mid_skip_keeps_the_decoding_track(
        Engine::Live(handing_over(
            Outgoing { from: 1.0 },
            Incoming::Loading(loading_track("/b")),
        )),
        opened(None, Duration::ZERO, Playback::Playing),
        Transition {
            next: Engine::Live(Live {
                phase: Phase::Loading(loading_track("/b")),
                config: EngineConfig { crossfade: crossfade(10), ..config() },
                ..live()
            }),
            io: EngineEffect::SetVolume(1.0),
        }
    )]
    #[case::a_decoded_skip_starts_and_ramps_over_the_retiring_stream(
        Engine::Live(handing_over(
            Outgoing { from: 0.5 },
            Incoming::Loading(loading_track("/b")),
        )),
        EngineMessage::Decoded(Ok(Some(PRELOAD_TOTAL))),
        Transition {
            next: Engine::Live(retiring(0.5)),
            io: EngineEffect::Many(vec![
                EngineEffect::Start { volume: 1.0, total: Some(PRELOAD_TOTAL) },
                EngineEffect::Ramp { length: secs(10), playing: 1.0 },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::a_failed_skip_drops_the_retiring_stream_too(
        Engine::Live(handing_over(
            Outgoing { from: 1.0 },
            Incoming::Loading(loading_track("/b")),
        )),
        EngineMessage::Decoded(Err(decode_fault())),
        Transition {
            next: Engine::Live(live_with_crossfade(CROSSFADE_SECONDS)),
            io: EngineEffect::Many(vec![
                EngineEffect::Clear,
                EngineEffect::Send(AudioEvent::Error(decode_fault())),
            ]),
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
    #[case::a_decode_after_stop_has_nothing_to_install(
        Engine::Live(live()),
        EngineMessage::Decoded(Ok(Some(TOTAL)))
    )]
    #[case::a_failed_decode_after_stop_is_not_reported(
        Engine::Live(live()),
        EngineMessage::Decoded(Err(decode_fault()))
    )]
    #[case::a_decode_after_the_skip_landed_has_nothing_to_install(
        Engine::Live(retiring(0.5)),
        EngineMessage::Decoded(Ok(Some(TOTAL)))
    )]
    fn a_stale_decode_leaves_the_engine_alone(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.update(message), Ok(EngineEffect::Nothing));
        assert_eq!(state, start);
    }

    #[test]
    fn a_stop_then_a_landed_decode_sends_nothing() {
        let mut engine = Engine::Live(loading());
        assert_eq!(engine.update(cmd(AudioCmd::Stop)), Ok(EngineEffect::Clear));
        assert_eq!(
            engine.update(EngineMessage::Decoded(Ok(Some(TOTAL)))),
            Ok(EngineEffect::Nothing)
        );
    }

    #[test]
    fn a_track_path_is_kept_for_the_resume() {
        let mut state = Engine::Live(playing());
        state
            .update(opened(Some("usb"), secs(5), Playback::Paused))
            .unwrap();
        let Engine::Live(Live {
            phase: Phase::Loading(loading),
            ..
        }) = state
        else {
            panic!("a reopened engine reloads its track, got {state:?}");
        };
        assert_eq!(loading.path, PathBuf::from("/a"));
    }
}
