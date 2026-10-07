use std::time::Duration;

use kernel::{
    cmd::Cmd,
    message::{AudioError, AudioEvent},
    update::machine::{LoopEffect, Unhandled},
};

use crate::engine::{
    effect::{AudioLoopCmd, EngineEffect},
    message::DeviceOpened,
    phase::{
        Incoming,
        LoadedTrack,
        Loading,
        NextTrack,
        Phase,
        Playing,
        Resume,
        Upcoming,
    },
    revisions::JobRevisions,
    state::{Closed, Live, then_report},
};

impl Live {
    pub(crate) fn closed(self) -> Closed {
        Closed {
            settings: self.settings,
            track_load: None,
            speed: self.speed,
        }
    }

    pub(crate) fn opened(
        &mut self,
        revisions: &mut JobRevisions,
        device_opened: DeviceOpened,
    ) -> AudioLoopCmd {
        let DeviceOpened {
            device,
            position,
            playback,
        } = device_opened;
        self.settings.device = device;
        let (current, upcoming) = match std::mem::replace(&mut self.phase, Phase::Idle)
        {
            Phase::Idle => {
                return Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(
                    self.gain(),
                )));
            }
            Phase::Loading(loading) | Phase::Handover(Incoming::Loading(loading)) => {
                self.phase = Phase::Loading(loading);
                return Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(
                    self.gain(),
                )));
            }
            Phase::Playing(Playing { current, next }) => {
                let upcoming = match next {
                    NextTrack::None => None,
                    NextTrack::Preloading { path, decibels } => {
                        Some(Upcoming { path, decibels })
                    }
                    NextTrack::Gapless(track) => Some(Upcoming::from(track)),
                    NextTrack::Crossfading {
                        incoming,
                        fade: _fade,
                    } => Some(Upcoming::from(incoming)),
                };
                (current, upcoming)
            }
            Phase::Handover(Incoming::Playing(current)) => (current, None),
        };
        let LoadedTrack {
            duration,
            decibels,
            path,
        } = current;
        self.phase = Phase::Loading(Loading {
            path: path.clone(),
            decibels,
            resume: Some(Resume {
                position,
                playback,
                duration,
                upcoming,
            }),
        });
        Cmd::effect(LoopEffect::Execute(EngineEffect::ClearStaged))
            .then(Cmd::effect(LoopEffect::Run(revisions.decode_job(path))))
    }

    pub(crate) fn decoded(
        &mut self,
        revisions: &mut JobRevisions,
        duration: Option<Duration>,
    ) -> Result<AudioLoopCmd, Unhandled> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Loading(loading) => {
                let (current, mut after_load) = loading.into_current(duration);
                let (next, preload) = match after_load
                    .as_mut()
                    .and_then(|resume| resume.upcoming.take())
                {
                    None => (NextTrack::None, Cmd::none()),
                    Some(Upcoming { path, decibels }) => (
                        NextTrack::Preloading {
                            path: path.clone(),
                            decibels,
                        },
                        Cmd::effect(LoopEffect::Run(revisions.preload_job(path))),
                    ),
                };
                self.phase = Phase::Playing(Playing { current, next });
                Ok(then_report(self.start_effect(after_load.as_ref())).then(preload))
            }
            Phase::Handover(Incoming::Loading(loading)) => {
                let (current, after_load) = loading.into_current(duration);
                self.phase = Phase::Handover(Incoming::Playing(current));
                Ok(self.handover_started(after_load.as_ref()))
            }
            refused @ (Phase::Idle
            | Phase::Playing(_)
            | Phase::Handover(Incoming::Playing(_))) => {
                self.phase = refused;
                Err(Unhandled)
            }
        }
    }

    pub(crate) fn decode_failed(
        &mut self,
        revisions: &mut JobRevisions,
        error: AudioError,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let reported = Cmd::message(AudioEvent::Error(error));
        let cmd = match &self.phase {
            Phase::Loading(_) => reported,
            Phase::Handover(Incoming::Loading(_)) => {
                revisions.cancel();
                Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(self.speed)))
                    .then(reported)
            }
            Phase::Idle | Phase::Playing(_) | Phase::Handover(Incoming::Playing(_)) => {
                return Err(Unhandled);
            }
        };
        self.phase = Phase::Idle;
        Ok(cmd)
    }

    fn start_effect(&self, resume: Option<&Resume>) -> AudioLoopCmd {
        let gain = self.gain();
        match resume {
            None => Cmd::effect(LoopEffect::Execute(EngineEffect::Start(gain))).then(
                Cmd::message(AudioEvent::Loaded(
                    self.phase.current().and_then(|current| current.duration),
                )),
            ),
            Some(Resume {
                position,
                playback,
                duration: _duration,
                upcoming: _upcoming,
            }) => Cmd::effect(LoopEffect::Execute(EngineEffect::Resume {
                gain,
                position: *position,
                playback: *playback,
            })),
        }
    }

    fn handover_started(&self, resume: Option<&Resume>) -> AudioLoopCmd {
        then_report(
            self.start_effect(resume)
                .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Ramp {
                    duration: self.settings.crossfade.get(),
                    current: self.gain(),
                }))),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        cmd::{AudioCmd, Cmd, Playback},
        domain::{settings::AudioSettings, speed::Speed},
        message::AudioEvent,
        update::machine::{LoopEffect, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        deck::job::AudioJob,
        engine::{
            effect::EngineEffect,
            message::EngineMessage,
            phase::{Fade, Incoming, Loading, NextTrack, Phase, Playing, Resume},
            state::{Closed, EngineState, Live, then_report},
            tests::{
                CROSSFADE_SECONDS,
                EngineRow,
                TRACK_A_DURATION,
                TRACK_B_DURATION,
                assert_cell,
                assert_fallback,
                assert_same,
                cmd,
                crossfade,
                decode_error,
                decoding,
                error,
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
                settings,
                settings_on,
                step,
                trace,
                track_a,
                track_b,
            },
        },
    };

    #[rstest]
    #[case::live_adopts_the_device_that_actually_opened(
        EngineState::Live(Live { settings: settings_on("usb"), ..live() }),
        opened(settings().device, Duration::ZERO, Playback::Playing),
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(crate::gain::Gain::UNITY))))}
    )]
    #[case::open_failure_mutes_the_engine(
        EngineState::Live(playing()),
        EngineMessage::Error(error()),
        EngineRow {
            next: EngineState::Closed(Closed {
                settings: settings(),
                track_load: None,
                speed: Speed::default(),
            }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Silence))
                .then(Cmd::message(AudioEvent::Error(error())))),
        }
    )]
    #[case::reopened_resumes_the_current_track(
        EngineState::Live(playing()),
        opened(settings_on("usb").device, seconds(5), Playback::Paused),
        EngineRow { next: EngineState::Live(resuming()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::ClearStaged)).then(decoding("/a")))}
    )]
    #[case::reopened_keeps_a_pending_load(
        EngineState::Live(loading()),
        opened(settings_on("usb").device, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: EngineState::Live(Live { settings: settings_on("usb"), ..loading() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(crate::gain::Gain::UNITY)))),
        }
    )]
    #[case::reopened_with_nothing_loaded(
        EngineState::Live(live()),
        opened(settings().device, Duration::ZERO, Playback::Playing),
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(crate::gain::Gain::UNITY))))}
    )]
    #[case::decoded_starts_the_track(
        EngineState::Live(loading()),
        EngineMessage::Decoded(Some(TRACK_A_DURATION)),
        EngineRow {
            next: EngineState::Live(Live { phase: Phase::Playing(Playing::new(track_a())), ..playing() }),
            effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Start(crate::gain::Gain::UNITY))).then(Cmd::message(AudioEvent::Loaded(Some(TRACK_A_DURATION)))))),
        }
    )]
    #[case::decoded_resumes_where_the_old_device_was(
        EngineState::Live(resuming()),
        EngineMessage::Decoded(Some(TRACK_B_DURATION)),
        EngineRow {
            next: EngineState::Live(Live {
                phase: Phase::Playing(Playing::new(track_a())),
                settings: settings_on("usb"),
                ..live()
            }),
            effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Resume { gain: crate::gain::Gain::UNITY, position: seconds(5), playback: Playback::Paused },)))),
        }
    )]
    #[case::decode_failure_is_reported(
        EngineState::Live(loading()),
        EngineMessage::Error(decode_error()),
        EngineRow {
            next: EngineState::Live(live()),
            effect: Ok(Cmd::message(AudioEvent::Error(decode_error()))),
        }
    )]
    #[case::reopened_mid_skip_resumes_the_incoming_track(
        EngineState::Live(handed_over_to_b()),
        opened(settings_on("usb").device, seconds(5), Playback::Paused),
        EngineRow {
            next: EngineState::Live(Live {
                phase: Phase::Loading(Loading {
                    path: "/b".into(),
                    decibels: None,
                    resume: Some(Resume {
                        position: seconds(5),
                        playback: Playback::Paused,
                        duration: Some(TRACK_B_DURATION),
                        upcoming: None,
                    }),
                }),
                settings: AudioSettings { crossfade: crossfade(10), ..settings_on("usb") },
                ..live()
            }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::ClearStaged)).then(decoding("/b"))),
        }
    )]
    #[case::reopened_mid_skip_keeps_the_decoding_track(
        EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        opened(settings().device, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: EngineState::Live(Live {
                phase: Phase::Loading(loading_track("/b")),
                settings: AudioSettings { crossfade: crossfade(10), ..settings() },
                ..live()
            }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(crate::gain::Gain::UNITY)))),
        }
    )]
    #[case::a_decoded_skip_starts_and_ramps_over_the_outgoing_stream(
        EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Decoded(Some(TRACK_B_DURATION)),
        EngineRow {
            next: EngineState::Live(handed_over_to_b()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Start(crate::gain::Gain::UNITY))).then(Cmd::message(AudioEvent::Loaded(Some(TRACK_B_DURATION)))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Ramp { duration: seconds(10), current: crate::gain::Gain::UNITY }))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))),
        }
    )]
    #[case::a_failed_skip_drops_the_outgoing_stream_too(
        EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Error(decode_error()),
        EngineRow {
            next: EngineState::Live(live_with_crossfade(CROSSFADE_SECONDS)),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(Speed::default()))).then(Cmd::message(AudioEvent::Error(decode_error())))),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] engine_state: EngineState,
        #[case] message: EngineMessage,
        #[case] moved_row: EngineRow,
    ) {
        assert_cell(engine_state, message, moved_row);
    }

    #[test]
    fn a_live_engine_tells_the_world_the_device_fell_back() {
        assert_fallback(
            EngineState::Live(Live {
                settings: settings_on("usb"),
                ..live()
            }),
            EngineRow {
                next: EngineState::Live(live()),
                effect: Ok(Cmd::message(AudioEvent::DeviceFellBack(
                    kernel::domain::device::OutputDevice::SystemDefault,
                ))
                .then(Cmd::effect(LoopEffect::Execute(
                    EngineEffect::SetGain(crate::gain::Gain::UNITY),
                )))),
            },
        );
    }

    #[rstest]
    #[case::while_preloading(NextTrack::Preloading { path: "/b".into(), decibels: None })]
    #[case::while_gapless(NextTrack::Gapless(track_b()))]
    #[case::while_crossfading(NextTrack::Crossfading { incoming: track_b(), fade: Fade::Running })]
    fn a_reopened_device_preloads_the_upcoming_track_again(#[case] next: NextTrack) {
        let (engine_state, log) = trace(
            EngineState::Live(Live {
                phase: Phase::Playing(Playing {
                    current: track_a(),
                    next,
                }),
                ..live()
            }),
            vec![
                opened(settings().device, seconds(5), Playback::Playing),
                EngineMessage::Decoded(Some(TRACK_A_DURATION)),
            ],
        )
        .unwrap();
        assert_eq!(
            engine_state,
            EngineState::Live(Live {
                phase: Phase::Playing(Playing {
                    current: track_a(),
                    next: NextTrack::Preloading {
                        path: "/b".into(),
                        decibels: None,
                    },
                }),
                ..live()
            })
        );
        assert!(log.iter().flat_map(Cmd::effects).any(|effect| matches!(
            effect,
            LoopEffect::Run(AudioJob::Preload { path, .. }) if path.as_path() == std::path::Path::new("/b")
        )));
    }

    #[rstest]
    #[case::a_decode_after_stop_has_nothing_to_install(
        EngineState::Live(live()),
        EngineMessage::Decoded(Some(TRACK_A_DURATION))
    )]
    #[case::a_failed_decode_after_stop_is_not_reported(
        EngineState::Live(live()),
        EngineMessage::Error(decode_error())
    )]
    #[case::a_decode_after_the_skip_landed_has_nothing_to_install(
        EngineState::Live(handed_over_to_b()),
        EngineMessage::Decoded(Some(TRACK_A_DURATION))
    )]
    fn a_stale_decode_leaves_the_engine_alone(
        #[case] engine_state: EngineState,
        #[case] message: EngineMessage,
    ) {
        let mut state = engine_state.clone();
        assert_eq!(step(&mut state, message).err(), Some(Unhandled));
        assert_eq!(state, engine_state);
    }

    #[test]
    fn a_stop_then_a_landed_decode_sends_nothing() {
        let mut engine_state = EngineState::Live(loading());
        assert_same(
            step(&mut engine_state, cmd(AudioCmd::Stop)),
            Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(
                Speed::default(),
            )))),
        );
        assert_eq!(
            step(
                &mut engine_state,
                EngineMessage::Decoded(Some(TRACK_A_DURATION)),
            )
            .err(),
            Some(Unhandled)
        );
    }
}
