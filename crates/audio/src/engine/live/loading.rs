use std::time::Duration;

use kernel::{AudioError, AudioEvent, Cmd, update::Unhandled};

use crate::{
    deck::DeviceOpened,
    engine::{
        effect::EngineEffect,
        phase::{CurrentTrack, Handover, Incoming, Loading, Phase, Playing, Resume},
        state::{Live, Muted, announce, then_report},
    },
};

impl Live {
    pub(crate) fn failed(&self) -> Muted {
        Muted {
            settings: self.settings.clone(),
            pending: None,
            speed: self.speed,
        }
    }

    pub(crate) fn opened(
        &mut self,
        reopened: DeviceOpened,
    ) -> Cmd<EngineEffect, AudioEvent> {
        let DeviceOpened {
            device,
            position,
            playback,
            opened,
        } = reopened;
        self.settings.device = device.clone();
        let effect = match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => Cmd::effect(EngineEffect::SetVolume(self.volume())),
            Phase::Loading(loading)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(loading),
            }) => {
                self.phase = Phase::Loading(loading);
                Cmd::effect(EngineEffect::SetVolume(self.volume()))
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
                Cmd::effect(EngineEffect::Decode(path))
            }
        };
        announce(opened, device, effect)
    }

    pub(crate) fn decoded(
        &mut self,
        result: Result<Option<Duration>, AudioError>,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        Ok(match (&self.phase, result) {
            (Phase::Loading(_), Err(error)) => {
                self.phase = Phase::Idle;
                Cmd::message(AudioEvent::Error(error))
            }
            (
                Phase::Handover(Handover {
                    incoming: Incoming::Loading(_),
                }),
                Err(error),
            ) => {
                self.phase = Phase::Idle;
                Cmd::effect(EngineEffect::Clear)
                    .then(Cmd::message(AudioEvent::Error(error)))
            }
            (Phase::Loading(loading), Ok(total)) => {
                let (current, after_load) = loading.clone().into_current(total);
                self.phase = Phase::Playing(Playing::new(current));
                then_report(self.start_effect(after_load.as_ref()))
            }
            (
                Phase::Handover(Handover {
                    incoming: Incoming::Loading(loading),
                }),
                Ok(total),
            ) => {
                let (current, after_load) = loading.clone().into_current(total);
                self.phase = Phase::Handover(Handover {
                    incoming: Incoming::Playing(current),
                });
                self.handover_started(after_load.as_ref())
            }
            (
                Phase::Idle
                | Phase::Playing(_)
                | Phase::Handover(Handover {
                    incoming: Incoming::Playing(_),
                }),
                Ok(_) | Err(_),
            ) => return Err(Unhandled),
        })
    }

    fn start_effect(
        &self,
        after_load: Option<&Resume>,
    ) -> Cmd<EngineEffect, AudioEvent> {
        let volume = self.volume();
        match after_load {
            None => Cmd::effect(EngineEffect::Start(volume)).then(Cmd::message(
                AudioEvent::Loaded(
                    self.phase.current().and_then(|current| current.total),
                ),
            )),
            Some(Resume {
                position, playback, ..
            }) => Cmd::effect(EngineEffect::Resume {
                volume,
                position: *position,
                playback: *playback,
            }),
        }
    }

    fn handover_started(
        &self,
        after_load: Option<&Resume>,
    ) -> Cmd<EngineEffect, AudioEvent> {
        let volume = self.volume();
        let length = self.settings.crossfade.get();
        let start = self.start_effect(after_load);
        start
            .then(Cmd::effect(EngineEffect::Ramp {
                length,
                playing: volume,
            }))
            .then(Cmd::effect(EngineEffect::Report))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        AudioCmd,
        AudioEvent,
        Cmd,
        Playback,
        domain::{AudioSettings, Speed},
        update::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::engine::{
        effect::{AudioMessage, EngineEffect},
        phase::{Incoming, Loading, Phase, Playing, Resume},
        state::{Engine, Live, Muted, then_report},
        tests::{
            CROSSFADE_SECONDS,
            EngineRow,
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
    };

    #[rstest]
    #[case::live_adopts_the_device_that_actually_opened(
        Engine::Live(Live { settings: config_on("usb"), ..live() }),
        opened(config().device, Duration::ZERO, Playback::Playing),
        EngineRow { next: Engine::Live(live()), effect: Cmd::effect(EngineEffect::SetVolume(1.0))}
    )]
    #[case::a_live_engine_tells_the_world_the_device_fell_back(
        Engine::Live(Live { settings: config_on("usb"), ..live() }),
        fell_back(Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(live()),
            effect: Cmd::message(AudioEvent::DeviceFellBack(kernel::domain::OutputDevice::SystemDefault)).then(Cmd::effect(EngineEffect::SetVolume(1.0))),
        }
    )]
    #[case::open_failure_mutes_the_engine(
        Engine::Live(playing()),
        AudioMessage::Opened(Err(error())),
        EngineRow {
            next: Engine::Muted(Muted {
                settings: config(),
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::effect(EngineEffect::Mute)
                .then(Cmd::message(AudioEvent::Error(error()))),
        }
    )]
    #[case::reopened_resumes_the_current_track(
        Engine::Live(playing()),
        opened(config_on("usb").device, seconds(5), Playback::Paused),
        EngineRow { next: Engine::Live(resuming()), effect: Cmd::effect(EngineEffect::Decode("/a".into()))}
    )]
    #[case::reopened_keeps_a_pending_load(
        Engine::Live(loading()),
        opened(config_on("usb").device, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(Live { settings: config_on("usb"), ..loading() }),
            effect: Cmd::effect(EngineEffect::SetVolume(1.0)),
        }
    )]
    #[case::reopened_with_nothing_loaded(
        Engine::Live(live()),
        opened(config().device, Duration::ZERO, Playback::Playing),
        EngineRow { next: Engine::Live(live()), effect: Cmd::effect(EngineEffect::SetVolume(1.0))}
    )]
    #[case::decoded_starts_the_track(
        Engine::Live(loading()),
        AudioMessage::Decoded(Ok(Some(TOTAL))),
        EngineRow {
            next: Engine::Live(Live { phase: Phase::Playing(Playing::new(track_a())), ..playing() }),
            effect: then_report(Cmd::effect(EngineEffect::Start(1.0)).then(Cmd::message(AudioEvent::Loaded(Some(TOTAL))))),
        }
    )]
    #[case::decoded_resumes_where_the_old_device_was(
        Engine::Live(resuming()),
        AudioMessage::Decoded(Ok(Some(PRELOAD_TOTAL))),
        EngineRow {
            next: Engine::Live(Live {
                phase: Phase::Playing(Playing::new(track_a())),
                settings: config_on("usb"),
                ..live()
            }),
            effect: then_report(Cmd::effect(EngineEffect::Resume { volume: 1.0, position: seconds(5), playback: Playback::Paused },)),
        }
    )]
    #[case::decode_failure_is_reported(
        Engine::Live(loading()),
        AudioMessage::Decoded(Err(decode_error())),
        EngineRow {
            next: Engine::Live(live()),
            effect: Cmd::message(AudioEvent::Error(decode_error())),
        }
    )]
    #[case::reopened_mid_skip_resumes_the_incoming_track(
        Engine::Live(handed_over_to_b()),
        opened(config_on("usb").device, seconds(5), Playback::Paused),
        EngineRow {
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
                settings: AudioSettings { crossfade: crossfade(10), ..config_on("usb") },
                ..live()
            }),
            effect: Cmd::effect(EngineEffect::Decode("/b".into())),
        }
    )]
    #[case::reopened_mid_skip_keeps_the_decoding_track(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        opened(config().device, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(Live {
                phase: Phase::Loading(loading_track("/b")),
                settings: AudioSettings { crossfade: crossfade(10), ..config() },
                ..live()
            }),
            effect: Cmd::effect(EngineEffect::SetVolume(1.0)),
        }
    )]
    #[case::a_decoded_skip_starts_and_ramps_over_the_outgoing_stream(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        AudioMessage::Decoded(Ok(Some(PRELOAD_TOTAL))),
        EngineRow {
            next: Engine::Live(handed_over_to_b()),
            effect: Cmd::effect(EngineEffect::Start(1.0)).then(Cmd::message(AudioEvent::Loaded(Some(PRELOAD_TOTAL)))).then(Cmd::effect(EngineEffect::Ramp { length: seconds(10), playing: 1.0 })).then(Cmd::effect(EngineEffect::Report)),
        }
    )]
    #[case::a_failed_skip_drops_the_outgoing_stream_too(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        AudioMessage::Decoded(Err(decode_error())),
        EngineRow {
            next: Engine::Live(live_with_crossfade(CROSSFADE_SECONDS)),
            effect: Cmd::effect(EngineEffect::Clear).then(Cmd::message(AudioEvent::Error(decode_error()))),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: AudioMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
    }

    #[rstest]
    #[case::a_decode_after_stop_has_nothing_to_install(
        Engine::Live(live()),
        AudioMessage::Decoded(Ok(Some(TOTAL)))
    )]
    #[case::a_failed_decode_after_stop_is_not_reported(
        Engine::Live(live()),
        AudioMessage::Decoded(Err(decode_error()))
    )]
    #[case::a_decode_after_the_skip_landed_has_nothing_to_install(
        Engine::Live(handed_over_to_b()),
        AudioMessage::Decoded(Ok(Some(TOTAL)))
    )]
    fn a_stale_decode_leaves_the_engine_alone(
        #[case] start: Engine,
        #[case] message: AudioMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.transition(message), Err(Unhandled));
        assert_eq!(state, start);
    }

    #[test]
    fn a_stop_then_a_landed_decode_sends_nothing() {
        let mut engine = Engine::Live(loading());
        assert_eq!(
            engine.transition(cmd(AudioCmd::Stop)),
            Ok(Cmd::effect(EngineEffect::Clear))
        );
        assert_eq!(
            engine.transition(AudioMessage::Decoded(Ok(Some(TOTAL)))),
            Err(Unhandled)
        );
    }
}
