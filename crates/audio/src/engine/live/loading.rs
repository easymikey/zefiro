use std::time::Duration;

use kernel::{AudioError, AudioEvent};

use crate::{
    deck::DeviceOpened,
    engine::{
        effect::EngineEffect,
        phase::{CurrentTrack, Handover, Incoming, Loading, Phase, Playing, Resume},
        state::{Live, Muted, announce, then_report},
    },
};

impl Live {
    pub(crate) fn failed(&self, error: AudioError) -> (Muted, EngineEffect) {
        let muted = Muted {
            error: error.clone(),
            config: self.config.clone(),
            pending: None,
            speed: self.speed,
        };
        (muted, EngineEffect::Mute(error))
    }

    pub(crate) fn opened(&mut self, reopened: DeviceOpened) -> EngineEffect {
        let DeviceOpened {
            device,
            position,
            playback,
            opened,
        } = reopened;
        self.config.device = device.clone();
        let effect = match std::mem::take(&mut self.phase) {
            Phase::Idle => EngineEffect::SetVolume(self.volume()),
            Phase::Loading(loading)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(loading),
            }) => {
                self.phase = Phase::Loading(loading);
                EngineEffect::SetVolume(self.volume())
            }
            Phase::Playing(Playing { current, .. })
            | Phase::Handover(Handover {
                incoming: Incoming::Playing(current),
            }) => {
                let CurrentTrack { total, gain, path } = current;
                self.phase = Phase::Loading(Loading {
                    path: path.clone(),
                    gain,
                    after_load: Some(Resume {
                        position,
                        playback,
                        total,
                    }),
                });
                EngineEffect::Decode(path)
            }
        };
        announce(opened, device, effect)
    }

    pub(crate) fn decoded(
        &mut self,
        outcome: Result<Option<Duration>, AudioError>,
    ) -> EngineEffect {
        match (std::mem::take(&mut self.phase), outcome) {
            (Phase::Loading(_), Err(error)) => {
                EngineEffect::Send(AudioEvent::Error(error))
            }
            (
                Phase::Handover(Handover {
                    incoming: Incoming::Loading(_),
                }),
                Err(error),
            ) => EngineEffect::Batch(vec![
                EngineEffect::Clear,
                EngineEffect::Send(AudioEvent::Error(error)),
            ]),
            (Phase::Loading(loading), Ok(total)) => {
                let (current, after_load) = loading.into_current(total);
                self.phase = Phase::Playing(Playing::new(current));
                then_report(self.start_effect(after_load.as_ref()))
            }
            (
                Phase::Handover(Handover {
                    incoming: Incoming::Loading(loading),
                }),
                Ok(total),
            ) => {
                let (current, after_load) = loading.into_current(total);
                self.phase = Phase::Handover(Handover {
                    incoming: Incoming::Playing(current),
                });
                self.handover_started(after_load.as_ref())
            }
            (
                phase @ (Phase::Idle
                | Phase::Playing(_)
                | Phase::Handover(Handover {
                    incoming: Incoming::Playing(_),
                })),
                Ok(_) | Err(_),
            ) => {
                self.phase = phase;
                EngineEffect::Nothing
            }
        }
    }

    fn start_effect(&self, after_load: Option<&Resume>) -> EngineEffect {
        let volume = self.volume();
        match after_load {
            None => EngineEffect::Start {
                volume,
                total: self.phase.current().and_then(|current| current.total),
            },
            Some(Resume {
                position, playback, ..
            }) => EngineEffect::Resume {
                volume,
                position: *position,
                playback: *playback,
            },
        }
    }

    fn handover_started(&self, after_load: Option<&Resume>) -> EngineEffect {
        let volume = self.volume();
        let length = self.config.crossfade.get();
        let start = self.start_effect(after_load);
        EngineEffect::Batch(vec![
            start,
            EngineEffect::Ramp {
                length,
                playing: volume,
            },
            EngineEffect::Report,
        ])
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{AudioCmd, AudioEvent, Playback, domain::Speed, update::Machine};
    use rstest::rstest;

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage},
            phase::{Incoming, Loading, Phase, Playing, Resume},
            state::{Engine, Live, Muted, then_report},
            test_support::{
                CROSSFADE_SECONDS,
                Cell,
                PRELOAD_TOTAL,
                TOTAL,
                assert_cell,
                cmd,
                config,
                config_on,
                crossfade,
                decode_error,
                error,
                fell_back,
                handed_over_to_b,
                handing_over,
                live,
                live_with_crossfade,
                loading,
                loading_track,
                opened,
                playing,
                resuming,
                seconds,
                track_a,
            },
        },
    };

    #[rstest]
    #[case::live_adopts_the_device_that_actually_opened(
        Engine::Live(Live { config: config_on("usb"), ..live() }),
        opened(config().device, Duration::ZERO, Playback::Playing),
        Cell { next: Engine::Live(live()), effect: EngineEffect::SetVolume(1.0) }
    )]
    #[case::a_live_engine_tells_the_world_the_device_fell_back(
        Engine::Live(Live { config: config_on("usb"), ..live() }),
        fell_back(Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(live()),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Send(AudioEvent::DeviceFellBack(kernel::domain::OutputDevice::SystemDefault)),
                EngineEffect::SetVolume(1.0),
            ]),
        }
    )]
    #[case::open_failure_mutes_the_engine(
        Engine::Live(playing()),
        EngineMessage::Opened(Err(error())),
        Cell {
            next: Engine::Muted(Muted {
                error: error(),
                config: config(),
                pending: None,
                speed: Speed::default(),
            }),
            effect: EngineEffect::Mute(error()),
        }
    )]
    #[case::reopened_resumes_the_current_track(
        Engine::Live(playing()),
        opened(config_on("usb").device, seconds(5), Playback::Paused),
        Cell { next: Engine::Live(resuming()), effect: EngineEffect::Decode("/a".into()) }
    )]
    #[case::reopened_keeps_a_pending_load(
        Engine::Live(loading()),
        opened(config_on("usb").device, Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(Live { config: config_on("usb"), ..loading() }),
            effect: EngineEffect::SetVolume(1.0),
        }
    )]
    #[case::reopened_with_nothing_loaded(
        Engine::Live(live()),
        opened(config().device, Duration::ZERO, Playback::Playing),
        Cell { next: Engine::Live(live()), effect: EngineEffect::SetVolume(1.0) }
    )]
    #[case::decoded_starts_the_track(
        Engine::Live(loading()),
        EngineMessage::Decoded(Ok(Some(TOTAL))),
        Cell {
            next: Engine::Live(Live { phase: Phase::Playing(Playing::new(track_a())), ..playing() }),
            effect: then_report(EngineEffect::Start { volume: 1.0, total: Some(TOTAL) }),
        }
    )]
    #[case::decoded_resumes_where_the_old_device_was(
        Engine::Live(resuming()),
        EngineMessage::Decoded(Ok(Some(PRELOAD_TOTAL))),
        Cell {
            next: Engine::Live(Live {
                phase: Phase::Playing(Playing::new(track_a())),
                config: config_on("usb"),
                ..live()
            }),
            effect: then_report(
                EngineEffect::Resume { volume: 1.0, position: seconds(5), playback: Playback::Paused },
            ),
        }
    )]
    #[case::decode_failure_is_reported(
        Engine::Live(loading()),
        EngineMessage::Decoded(Err(decode_error())),
        Cell {
            next: Engine::Live(live()),
            effect: EngineEffect::Send(AudioEvent::Error(decode_error())),
        }
    )]
    #[case::reopened_mid_skip_resumes_the_incoming_track(
        Engine::Live(handed_over_to_b()),
        opened(config_on("usb").device, seconds(5), Playback::Paused),
        Cell {
            next: Engine::Live(Live {
                phase: Phase::Loading(Loading {
                    path: "/b".into(),
                    gain: None,
                    after_load: Some(Resume {
                        position: seconds(5),
                        playback: Playback::Paused,
                        total: Some(PRELOAD_TOTAL),
                    }),
                }),
                config: EngineConfig { crossfade: crossfade(10), ..config_on("usb") },
                ..live()
            }),
            effect: EngineEffect::Decode("/b".into()),
        }
    )]
    #[case::reopened_mid_skip_keeps_the_decoding_track(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        opened(config().device, Duration::ZERO, Playback::Playing),
        Cell {
            next: Engine::Live(Live {
                phase: Phase::Loading(loading_track("/b")),
                config: EngineConfig { crossfade: crossfade(10), ..config() },
                ..live()
            }),
            effect: EngineEffect::SetVolume(1.0),
        }
    )]
    #[case::a_decoded_skip_starts_and_ramps_over_the_outgoing_stream(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Decoded(Ok(Some(PRELOAD_TOTAL))),
        Cell {
            next: Engine::Live(handed_over_to_b()),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Start { volume: 1.0, total: Some(PRELOAD_TOTAL) },
                EngineEffect::Ramp { length: seconds(10), playing: 1.0 },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::a_failed_skip_drops_the_outgoing_stream_too(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Decoded(Err(decode_error())),
        Cell {
            next: Engine::Live(live_with_crossfade(CROSSFADE_SECONDS)),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Clear,
                EngineEffect::Send(AudioEvent::Error(decode_error())),
            ]),
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
    #[case::a_decode_after_stop_has_nothing_to_install(
        Engine::Live(live()),
        EngineMessage::Decoded(Ok(Some(TOTAL)))
    )]
    #[case::a_failed_decode_after_stop_is_not_reported(
        Engine::Live(live()),
        EngineMessage::Decoded(Err(decode_error()))
    )]
    #[case::a_decode_after_the_skip_landed_has_nothing_to_install(
        Engine::Live(handed_over_to_b()),
        EngineMessage::Decoded(Ok(Some(TOTAL)))
    )]
    fn a_stale_decode_leaves_the_engine_alone(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.transition(message), Ok(EngineEffect::Nothing));
        assert_eq!(state, start);
    }

    #[test]
    fn a_stop_then_a_landed_decode_sends_nothing() {
        let mut engine = Engine::Live(loading());
        assert_eq!(
            engine.transition(cmd(AudioCmd::Stop)),
            Ok(EngineEffect::Clear)
        );
        assert_eq!(
            engine.transition(EngineMessage::Decoded(Ok(Some(TOTAL)))),
            Ok(EngineEffect::Nothing)
        );
    }
}
