use std::time::Duration;

use kernel::{
    cmd::Cmd,
    message::{AudioError, AudioEvent},
    update::machine::{LoopEffect, Unhandled},
};

use crate::{
    deck::source::PreloadMode,
    engine::{
        crossfade::{fade_start, replay_gain_factor},
        effect::{AudioLoopCmd, EngineEffect},
        message::SinkRole,
        phase::{Fade, Incoming, LoadedTrack, NextTrack, Phase, Playing},
        state::{Live, then_report},
    },
};

impl Live {
    pub(crate) fn finished(
        &mut self,
        role: SinkRole,
    ) -> Result<AudioLoopCmd, Unhandled> {
        self.settled(role, Live::finished_current)
    }

    pub(crate) fn ramped(&mut self, role: SinkRole) -> Result<AudioLoopCmd, Unhandled> {
        self.settled(role, Live::ramped_current)
    }

    fn settled<F>(
        &mut self,
        role: SinkRole,
        current: F,
    ) -> Result<AudioLoopCmd, Unhandled>
    where
        F: FnOnce(&mut Live) -> Result<AudioLoopCmd, Unhandled>,
    {
        match role {
            SinkRole::Current => current(self),
            SinkRole::Outgoing => self.handover_settled(),
            SinkRole::Incoming => Err(Unhandled),
        }
    }

    fn finished_current(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        if let Phase::Handover(Incoming::Playing(_)) = self.phase {
            self.phase = Phase::Idle;
            return Ok(then_report(
                Cmd::effect(LoopEffect::Execute(EngineEffect::DropOutgoing))
                    .then(Cmd::message(AudioEvent::Ended)),
            ));
        }
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        match std::mem::replace(&mut playing.next, NextTrack::None) {
            NextTrack::Gapless(incoming) => {
                playing.current = incoming;
                Ok(then_report(
                    Cmd::effect(LoopEffect::Execute(EngineEffect::Advance(
                        self.gain(),
                    )))
                    .then(Cmd::message(AudioEvent::TrackChanged)),
                ))
            }
            NextTrack::None | NextTrack::Preloading { .. } => {
                self.phase = Phase::Idle;
                Ok(then_report(Cmd::message(AudioEvent::Ended)))
            }
            NextTrack::Crossfading {
                incoming,
                fade: _fade,
            } => {
                playing.current = incoming;
                Ok(self.promoted())
            }
        }
    }

    pub(crate) fn fade_start_reached(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(Playing {
            next: NextTrack::Crossfading { incoming, fade },
            current: _current,
        }) = &mut self.phase
        else {
            return Err(Unhandled);
        };
        if *fade == Fade::Running {
            return Err(Unhandled);
        }
        *fade = Fade::Running;
        Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Crossfade {
            duration: self.settings.crossfade.get(),
            incoming: replay_gain_factor(self.settings.replay_gain, incoming.decibels),
        })))
    }

    fn ramped_current(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        let NextTrack::Crossfading {
            fade: Fade::Running,
            incoming: _incoming,
        } = &playing.next
        else {
            return Err(Unhandled);
        };
        playing.promote();
        Ok(self.promoted())
    }

    fn handover_settled(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Handover(Incoming::Playing(current)) => {
                self.phase = Phase::Playing(Playing::new(current));
                Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::DropOutgoing)))
            }
            unsettled @ (Phase::Idle
            | Phase::Loading(_)
            | Phase::Playing(_)
            | Phase::Handover(Incoming::Loading(_))) => {
                self.phase = unsettled;
                Err(Unhandled)
            }
        }
    }

    pub(crate) fn preload_mode(&self) -> Option<PreloadMode> {
        let Phase::Playing(Playing {
            next: NextTrack::Preloading { .. },
            current: _current,
        }) = &self.phase
        else {
            return None;
        };
        Some(if self.settings.crossfade.get().is_zero() {
            PreloadMode::Gapless
        } else {
            PreloadMode::Crossfade(self.speed)
        })
    }

    pub(crate) fn attached(
        &mut self,
        preload_mode: PreloadMode,
        duration: Option<Duration>,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        let NextTrack::Preloading { media, decibels } = &playing.next else {
            return Err(Unhandled);
        };
        let incoming = LoadedTrack {
            duration,
            decibels: *decibels,
            media: media.clone(),
        };
        let (next, cmd) = match preload_mode {
            PreloadMode::Gapless => (NextTrack::Gapless(incoming), Cmd::none()),
            PreloadMode::Crossfade(_) => {
                let fade_start =
                    fade_start(playing.current.duration, self.settings.crossfade.get());
                let fade_start_cmd = fade_start.map_or_else(Cmd::none, |fade_start| {
                    Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(Some(
                        fade_start,
                    ))))
                });
                let fade = Fade::Armed;
                (NextTrack::Crossfading { incoming, fade }, fade_start_cmd)
            }
        };
        playing.next = next;
        Ok(cmd)
    }

    pub(crate) fn preload_failed(
        &mut self,
        error: AudioError,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(Playing {
            next: next @ NextTrack::Preloading { .. },
            current: _current,
        }) = &mut self.phase
        else {
            return Err(Unhandled);
        };
        *next = NextTrack::None;
        Ok(Cmd::message(AudioEvent::Error(error)))
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        cmd::{AudioCmd, Cmd, Media, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            revision::Revision,
            settings::{AudioSettings, ReplayGain},
            track::Decibels,
        },
        message::{AudioError, AudioEvent, DecodeError},
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use proptest::prelude::{prop_assert_eq, proptest};
    use rstest::rstest;
    use tempfile::NamedTempFile;

    use crate::{
        deck::{
            feed::{feed_channel, tests::corrupt_file},
            job::AudioJob,
            source::{DecodedTrack, PreloadMode, decode},
        },
        engine::{
            crossfade::replay_gain_factor,
            effect::EngineEffect,
            message::{AudioMessage, EngineMessage, SinkRole},
            phase::{Incoming, LoadedTrack, NextTrack, Phase, Playing},
            state::{Engine, EngineState, Live, then_report},
            tests::{
                CROSSFADE_SECONDS,
                EngineRow,
                TRACK_B_DURATION,
                assert_cell,
                assert_same,
                attached,
                awaiting,
                closed,
                cmd,
                crossfading_idle,
                crossfading_mid_ramp,
                first,
                handed_over_to_b,
                handing_over,
                live,
                loading,
                loading_track,
                playing,
                playing_track,
                playing_with_crossfade,
                preload,
                preload_error,
                preload_with_revision,
                promoted,
                second,
                seconds,
                set_crossfade,
                settings,
                step,
                trace,
                track_a,
                track_b,
                unhandled,
                with_preload_revision,
            },
        },
    };

    #[rstest]
    #[case::a_finished_current_moves_the_track(
        EngineState::Live(Live {
            phase: Phase::Playing(Playing { next: NextTrack::Gapless(track_b()), ..Playing::new(track_a()) }),
            ..live()
        }),
        EngineMessage::Finished(SinkRole::Current),
        EngineRow {
            next: EngineState::Live(Live { phase: Phase::Playing(Playing::new(track_b())), ..live() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Advance(crate::gain::Gain::UNITY)))
                .then(Cmd::message(AudioEvent::TrackChanged))
                .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))),
        }
    )]
    #[case::a_track_that_ends_inside_its_handover_ends(
        EngineState::Live(handed_over_to_b()),
        EngineMessage::Finished(SinkRole::Current),
        EngineRow {
            next: EngineState::Live(Live { phase: Phase::Idle, ..handed_over_to_b() }),
            effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::DropOutgoing)).then(Cmd::message(AudioEvent::Ended)))),
        }
    )]
    #[case::a_finish_mid_crossfade_promotes(
        EngineState::Live(crossfading_mid_ramp()),
        EngineMessage::Finished(SinkRole::Current),
        EngineRow {
            next: EngineState::Live(promoted(Crossfade::clamped(seconds(10)))),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Promote(crate::gain::Gain::UNITY)))
                .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))
                .then(Cmd::message(AudioEvent::TrackChanged))),
        }
    )]
    #[case::a_finished_ramp_promotes_the_incoming_track(
        EngineState::Live(crossfading_mid_ramp()),
        EngineMessage::Ramped(SinkRole::Current),
        EngineRow {
            next: EngineState::Live(promoted(Crossfade::clamped(seconds(10)))),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Promote(crate::gain::Gain::UNITY)))
                .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))
                .then(Cmd::message(AudioEvent::TrackChanged))),
        }
    )]
    #[case::a_track_at_its_fade_start_fades_in_and_out(
        EngineState::Live(crossfading_idle()),
        EngineMessage::FadeStartReached,
        EngineRow {
            next: EngineState::Live(crossfading_mid_ramp()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Crossfade { duration: seconds(10), incoming: crate::gain::Gain::UNITY }))),
        }
    )]
    #[case::ramped_outgoing_drops_it(
        EngineState::Live(handed_over_to_b()),
        EngineMessage::Ramped(SinkRole::Outgoing),
        EngineRow {
            next: EngineState::Live(Live { phase: playing_track(track_b()), ..handed_over_to_b() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::DropOutgoing))),
        }
    )]
    #[case::finished_outgoing_drops_it(
        EngineState::Live(handed_over_to_b()),
        EngineMessage::Finished(SinkRole::Outgoing),
        EngineRow {
            next: EngineState::Live(Live { phase: playing_track(track_b()), ..handed_over_to_b() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::DropOutgoing))),
        }
    )]
    #[case::preloaded_opens_the_crossfade(
        EngineState::Live(awaiting(
            playing_with_crossfade(),
            "/b",
        )),
        attached(&track_b(), Revision::default()),
        EngineRow {
            next: EngineState::Live(crossfading_idle()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(Some(seconds(90)))))),
        }
    )]
    #[case::a_failed_preload_is_reported(
        EngineState::Live(awaiting(playing(), "/b")),
        EngineMessage::Error(preload_error()),
        EngineRow {
            next: EngineState::Live(playing()),
            effect: Ok(Cmd::message(AudioEvent::Error(preload_error()))),
        }
    )]
    #[case::finished_while_idle_is_ignored(
        EngineState::Live(live()),
        EngineMessage::Finished(SinkRole::Current),
        EngineRow { next: EngineState::Live(live()), effect: Err(Unhandled) }
    )]
    #[case::finished_while_loading_is_ignored(
        EngineState::Live(loading()),
        EngineMessage::Finished(SinkRole::Current),
        EngineRow { next: EngineState::Live(loading()), effect: Err(Unhandled) }
    )]
    #[case::finished_while_closed_is_ignored(
        closed(),
        EngineMessage::Finished(SinkRole::Current),
        EngineRow { next: closed(), effect: Err(Unhandled) }
    )]
    #[case::finished_outgoing_without_a_handover_is_ignored(
        EngineState::Live(playing()),
        EngineMessage::Finished(SinkRole::Outgoing),
        EngineRow { next: EngineState::Live(playing()), effect: Err(Unhandled) }
    )]
    #[case::ramped_outgoing_without_a_handover_is_ignored(
        EngineState::Live(crossfading_mid_ramp()),
        EngineMessage::Ramped(SinkRole::Outgoing),
        EngineRow { next: EngineState::Live(crossfading_mid_ramp()), effect: Err(Unhandled) }
    )]
    #[case::ramped_current_before_the_fade_runs_is_ignored(
        EngineState::Live(crossfading_idle()),
        EngineMessage::Ramped(SinkRole::Current),
        EngineRow { next: EngineState::Live(crossfading_idle()), effect: Err(Unhandled) }
    )]
    #[case::ramped_current_in_handover_ignored(
        EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Ramped(SinkRole::Current),
        EngineRow {
            next: EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
            effect: Err(Unhandled),
        }
    )]
    #[case::a_second_fade_start_is_ignored(
        EngineState::Live(crossfading_mid_ramp()),
        EngineMessage::FadeStartReached,
        EngineRow { next: EngineState::Live(crossfading_mid_ramp()), effect: Err(Unhandled) }
    )]
    #[case::fade_start_while_idle_is_ignored(
        EngineState::Live(live()),
        EngineMessage::FadeStartReached,
        EngineRow { next: EngineState::Live(live()), effect: Err(Unhandled) }
    )]
    #[case::fade_start_while_loading_is_ignored(
        EngineState::Live(loading()),
        EngineMessage::FadeStartReached,
        EngineRow { next: EngineState::Live(loading()), effect: Err(Unhandled) }
    )]
    #[case::an_attach_nobody_awaits_is_ignored(
        EngineState::Live(playing()),
        attached(&track_b(), Revision::default()),
        EngineRow { next: EngineState::Live(playing()), effect: Err(Unhandled)}
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] engine_state: EngineState,
        #[case] message: EngineMessage,
        #[case] moved_row: EngineRow,
    ) {
        assert_cell(engine_state, message, moved_row);
    }

    fn still_live(state: EngineState) -> Live {
        match state {
            EngineState::Live(live) => live,
            EngineState::Closed(_) => panic!("the engine must stay live"),
        }
    }

    #[test]
    fn a_preload_still_decoding_holds_up_neither_a_pause_nor_a_deck_event() {
        let (mut state, log) = trace(
            EngineState::Live(playing()),
            vec![preload("/b"), cmd(AudioCmd::SetPlayback(Playback::Paused))],
        )
        .unwrap();
        assert_eq!(
            step(&mut state, EngineMessage::Finished(SinkRole::Incoming)).err(),
            Some(Unhandled)
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_preload_attach_after_a_stop_is_ignored() {
        let (mut state, log) = trace(
            EngineState::Live(playing_with_crossfade()),
            vec![preload("/b"), cmd(AudioCmd::Stop)],
        )
        .unwrap();
        assert_eq!(
            step(&mut state, attached(&track_b(), first())).err(),
            Some(Unhandled)
        );
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert_eq!(live.phase, Phase::Idle);
    }

    #[test]
    fn an_attached_after_the_engine_moved_on_is_refused() {
        let mut engine = Engine::new(EngineState::Live(playing()));
        assert!(engine.transition(preload("/b")).is_ok());
        assert!(
            engine
                .transition(preload_with_revision("/c", second()))
                .is_ok()
        );
        let attached = |revision| EngineMessage::Attached {
            revision,
            preload_mode: PreloadMode::Gapless,
            duration: None,
        };
        assert_eq!(engine.transition(attached(first())).err(), Some(Unhandled));
        assert_eq!(
            engine.state,
            EngineState::Live(awaiting(
                with_preload_revision(playing(), second()),
                "/c"
            ))
        );
        assert!(engine.transition(attached(second())).is_ok());
    }

    #[test]
    fn a_landed_gapless_preload_still_hands_over_at_the_end() {
        let (_, log) = trace(
            EngineState::Live(playing()),
            vec![
                preload("/b"),
                EngineMessage::Attached {
                    revision: first(),
                    preload_mode: PreloadMode::Gapless,
                    duration: None,
                },
                EngineMessage::Finished(SinkRole::Current),
            ],
        )
        .unwrap();
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_gapless_advance_applies_the_next_track_gain() {
        let gain = Some(Decibels(-6.0));
        let live = Live {
            settings: AudioSettings {
                replay_gain: ReplayGain::On,
                ..settings()
            },
            ..playing()
        };
        let (state, log) = trace(
            EngineState::Live(live),
            vec![
                cmd(AudioCmd::Preload(TrackLoad {
                    media: Media::Local("/b".into()),
                    decibels: gain,
                    revision: first(),
                })),
                EngineMessage::Attached {
                    revision: first(),
                    preload_mode: PreloadMode::Gapless,
                    duration: None,
                },
                EngineMessage::Finished(SinkRole::Current),
            ],
        )
        .unwrap();
        let advanced = Cmd::effect(LoopEffect::Execute(EngineEffect::Advance(
            replay_gain_factor(ReplayGain::On, gain),
        )))
        .then(Cmd::message(AudioEvent::TrackChanged))
        .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)));
        assert_same(log.last(), Some(&advanced));
        assert_eq!(
            still_live(state).phase.current().and_then(|c| c.decibels),
            gain
        );
    }

    #[test]
    fn a_gapless_advance_keeps_the_duration_so_the_next_crossfade_still_fades() {
        let (_, log) = trace(
            EngineState::Live(playing()),
            vec![
                preload("/b"),
                EngineMessage::Attached {
                    revision: first(),
                    preload_mode: PreloadMode::Gapless,
                    duration: Some(TRACK_B_DURATION),
                },
                EngineMessage::Finished(SinkRole::Current),
                set_crossfade(CROSSFADE_SECONDS),
                preload_with_revision("/c", second()),
                attached(&track_b(), second()),
            ],
        )
        .unwrap();
        assert_same(
            log.last(),
            Some(&Cmd::effect(LoopEffect::Execute(
                EngineEffect::SetFadeStart(Some(
                    TRACK_B_DURATION - seconds(CROSSFADE_SECONDS),
                )),
            ))),
        );
    }

    #[test]
    fn a_landed_crossfade_preload_still_hands_over() {
        let (state, log) = trace(
            EngineState::Live(playing_with_crossfade()),
            vec![preload("/b"), attached(&track_b(), first())],
        )
        .unwrap();
        let live = still_live(state);
        assert!(matches!(
            live.phase,
            Phase::Playing(Playing {
                next: NextTrack::Crossfading { .. },
                ..
            })
        ));
        insta::assert_debug_snapshot!(log);
    }

    proptest! {
        #[test]
        fn an_attached_preload_keeps_the_path_it_was_requested_with(
            requested in "[a-z]{1,8}",
        ) {
            let (state, log) = trace(
                EngineState::Live(playing()),
                vec![
                    preload(&requested),
                    EngineMessage::Attached {
                        revision: first(),
                        preload_mode: PreloadMode::Gapless,
                        duration: None,
                    },
                ],
            )
            .map_err(unhandled)?;
            prop_assert_eq!(format!("{:?}", log.last()), format!("{:?}", Some(Cmd::<LoopEffect<EngineEffect, AudioJob, AudioMessage>, AudioEvent>::none())));
            let EngineState::Live(live) = state else {
                return Err(unhandled("the engine stays live across a preload"));
            };
            prop_assert_eq!(
                live.phase,
                Phase::Playing(Playing {
                    next: NextTrack::Gapless(LoadedTrack {
                        media: Media::Local(requested.into()),
                        decibels: None,
                        duration: None,
                    }),
                    ..Playing::new(track_a())
                })
            );
        }
    }

    fn reported(file: &NamedTempFile) -> EngineMessage {
        let decoder = decode(file.path()).unwrap();
        let channels = decoder.channels();
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder,
        };
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(4);
        let (_source, mut feed) =
            feed_channel(decoded_track, channels, callback_sender);
        feed.prime();
        match callback_receiver.try_recv() {
            Ok(AudioMessage::Engine(report)) => report,
            unreported => panic!("the feed reports its corrupt stream: {unreported:?}"),
        }
    }

    fn corrupt(file: &NamedTempFile) -> AudioError {
        AudioError::Decode {
            path: file.path().to_path_buf(),
            error: DecodeError::Corrupt,
        }
    }

    #[test]
    fn a_corrupt_stream_in_play_raises_the_error_and_then_ends_as_at_its_end() {
        let file = corrupt_file(24, 0);
        let mut state = EngineState::Live(playing());
        assert_same(
            step(&mut state, reported(&file)),
            Ok(Cmd::message(AudioEvent::Error(corrupt(&file)))),
        );
        assert_eq!(state, EngineState::Live(playing()));
        assert_same(
            step(&mut state, EngineMessage::Finished(SinkRole::Current)),
            Ok(then_report(Cmd::message(AudioEvent::Ended))),
        );
        assert_eq!(state, EngineState::Live(live()));
    }

    #[test]
    fn a_corrupt_outgoing_stream_raises_the_error_and_keeps_the_incoming_load() {
        let file = corrupt_file(24, 0);
        let engine_state =
            EngineState::Live(handing_over(Incoming::Loading(loading_track("/b"))));
        assert_cell(
            engine_state.clone(),
            reported(&file),
            EngineRow {
                next: engine_state,
                effect: Ok(Cmd::message(AudioEvent::Error(corrupt(&file)))),
            },
        );
    }
}
