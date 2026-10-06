use std::time::Duration;

use kernel::{
    cmd::Cmd,
    message::{AudioError, AudioEvent},
    update::machine::{LoopEffect, Unhandled},
};

use crate::{
    deck::source::PreloadMode,
    engine::{
        crossfade::{arm_cue, replay_gain_factor},
        effect::{AudioLoopCmd, EngineEffect},
        message::SinkRole,
        phase::{CurrentTrack, Fade, Incoming, Next, Phase, Playing},
        state::{Live, then_report},
    },
};

impl Live {
    pub(crate) fn finished(
        &mut self,
        role: SinkRole,
    ) -> Result<AudioLoopCmd, Unhandled> {
        self.settled(role, Live::finished_primary)
    }

    pub(crate) fn ramped(&mut self, role: SinkRole) -> Result<AudioLoopCmd, Unhandled> {
        self.settled(role, Live::ramped_primary)
    }

    fn settled<F>(
        &mut self,
        role: SinkRole,
        primary: F,
    ) -> Result<AudioLoopCmd, Unhandled>
    where
        F: FnOnce(&mut Live) -> Result<AudioLoopCmd, Unhandled>,
    {
        match role {
            SinkRole::Primary => primary(self),
            SinkRole::Outgoing => self.handover_settled(),
            SinkRole::Incoming => Err(Unhandled),
        }
    }

    fn finished_primary(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        match std::mem::replace(&mut playing.next, Next::None) {
            Next::Gapless(current_track) => {
                playing.current = current_track;
                Ok(then_report(
                    Cmd::effect(LoopEffect::Execute(EngineEffect::Advance(
                        self.gain(),
                    )))
                    .then(Cmd::message(AudioEvent::TrackChanged)),
                ))
            }
            Next::None | Next::Preloading { .. } => {
                self.phase = Phase::Idle;
                Ok(then_report(Cmd::message(AudioEvent::Ended)))
            }
            Next::Crossfading { preload, .. } => {
                playing.current = preload;
                Ok(self.promoted())
            }
        }
    }

    pub(crate) fn cued(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(Playing {
            next: Next::Crossfading { preload, fade },
            ..
        }) = &mut self.phase
        else {
            return Err(Unhandled);
        };
        if *fade == Fade::Running {
            return Err(Unhandled);
        }
        *fade = Fade::Running;
        Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Crossfade {
            length: self.settings.crossfade.get(),
            incoming: replay_gain_factor(self.settings.replay_gain, preload.gain),
        })))
    }

    fn ramped_primary(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        let Next::Crossfading {
            fade: Fade::Running,
            ..
        } = playing.next
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
            next: Next::Preloading { .. },
            ..
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
        let Next::Preloading { path, gain } = &playing.next else {
            return Err(Unhandled);
        };
        let current_track = CurrentTrack {
            total: duration,
            gain: *gain,
            path: path.clone(),
        };
        let (next, cmd) = match preload_mode {
            PreloadMode::Gapless => (Next::Gapless(current_track), Cmd::none()),
            PreloadMode::Crossfade(_) => {
                let cue = arm_cue(playing.current.total, self.settings.crossfade.get());
                let armed = cue.map_or_else(Cmd::none, |cue| {
                    Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(Some(cue))))
                });
                let fade = Fade::Armed;
                (
                    Next::Crossfading {
                        preload: current_track,
                        fade,
                    },
                    armed,
                )
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
            next: next @ Next::Preloading { .. },
            ..
        }) = &mut self.phase
        else {
            return Err(Unhandled);
        };
        *next = Next::None;
        Ok(Cmd::message(AudioEvent::Error(error)))
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        cmd::{AudioCmd, Cmd, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            revision::Revision,
            settings::{AudioSettings, ReplayGain},
            track::Decibels,
        },
        message::AudioEvent,
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use proptest::prelude::{prop_assert_eq, proptest};
    use rstest::rstest;

    use crate::{
        deck::{job::AudioJob, source::PreloadMode},
        engine::{
            crossfade::replay_gain_factor,
            effect::{AudioLoopCmd, EngineEffect},
            message::{AudioMessage, EngineMessage, SinkRole},
            phase::{CurrentTrack, Incoming, Next, Phase, Playing},
            revisions::JobRevisions,
            state::{Engine, EngineState, Live},
            tests::{
                CROSSFADE_SECONDS,
                EngineRow,
                PRELOAD_TOTAL,
                assert_cell,
                assert_same,
                awaiting,
                closed,
                cmd,
                crossfading_idle,
                crossfading_mid_ramp,
                first,
                handed_over_to_b,
                handing_over,
                installed,
                live,
                loading,
                loading_track,
                playing,
                playing_track,
                playing_with_crossfade,
                preload,
                preload_at,
                preload_error,
                preloaded_at,
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
            },
        },
    };

    struct DeckEventRow {
        start: EngineState,
        expected: EngineState,
        effect: AudioLoopCmd,
    }

    fn gapless_queued() -> EngineState {
        EngineState::Live(Live {
            phase: Phase::Playing(Playing {
                next: Next::Gapless(track_b()),
                ..Playing::new(track_a())
            }),
            ..live()
        })
    }

    #[test]
    fn a_finished_primary_moves_the_track() {
        let rows = vec![
            DeckEventRow {
                start: gapless_queued(),
                expected: EngineState::Live(Live {
                    phase: Phase::Playing(Playing::new(track_b())),
                    ..live()
                }),
                effect: Cmd::effect(LoopEffect::Execute(EngineEffect::Advance(
                    crate::gain::Gain::UNITY,
                )))
                .then(Cmd::message(AudioEvent::TrackChanged))
                .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report))),
            },
            DeckEventRow {
                start: EngineState::Live(playing()),
                expected: EngineState::Live(live()),
                effect: Cmd::message(AudioEvent::Ended)
                    .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report))),
            },
        ];
        for row in rows {
            let mut state = row.start;
            let effect =
                step(&mut state, EngineMessage::Finished(SinkRole::Primary)).unwrap();
            assert_same(effect, row.effect);
            assert_eq!(state, row.expected);
        }
    }

    #[rstest]
    #[case::idle(EngineState::Live(live()))]
    #[case::loading(EngineState::Live(loading()))]
    #[case::closed(closed())]
    fn a_finished_outside_a_track_is_ignored(#[case] start: EngineState) {
        let expected = start.clone();
        let mut state = start;
        assert_same(
            step(&mut state, EngineMessage::Finished(SinkRole::Primary)),
            Err(Unhandled),
        );
        assert_eq!(state, expected);
    }

    #[rstest]
    #[case::outgoing(SinkRole::Outgoing)]
    #[case::incoming(SinkRole::Incoming)]
    fn a_finished_outgoing_or_incoming_without_a_handover_is_nothing(
        #[case] role: SinkRole,
    ) {
        let mut state = EngineState::Live(playing());
        assert_same(
            step(&mut state, EngineMessage::Finished(role)),
            Err(Unhandled),
        );
    }

    #[test]
    fn a_finish_mid_crossfade_promotes() {
        let mut state = EngineState::Live(crossfading_mid_ramp());
        let effect =
            step(&mut state, EngineMessage::Finished(SinkRole::Primary)).unwrap();
        assert_same(
            effect,
            Cmd::effect(LoopEffect::Execute(EngineEffect::Promote(
                crate::gain::Gain::UNITY,
            )))
            .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))
            .then(Cmd::message(AudioEvent::TrackChanged)),
        );
        assert_eq!(
            state,
            EngineState::Live(promoted(Crossfade::clamped(seconds(10))))
        );
    }

    #[test]
    fn a_cued_track_fades_in_and_out() {
        let mut state = EngineState::Live(crossfading_idle());
        let effect = step(&mut state, EngineMessage::Cued).unwrap();
        assert_same(
            effect,
            Cmd::effect(LoopEffect::Execute(EngineEffect::Crossfade {
                length: seconds(10),
                incoming: crate::gain::Gain::UNITY,
            })),
        );
        assert_eq!(state, EngineState::Live(crossfading_mid_ramp()));
    }

    #[test]
    fn a_second_cue_is_ignored() {
        let mut state = EngineState::Live(crossfading_mid_ramp());
        assert_same(step(&mut state, EngineMessage::Cued), Err(Unhandled));
        assert_eq!(state, EngineState::Live(crossfading_mid_ramp()));
    }

    #[rstest]
    #[case::idle(EngineState::Live(live()))]
    #[case::loading(EngineState::Live(loading()))]
    fn a_cue_while_idle_or_loading_is_ignored(#[case] start: EngineState) {
        let expected = start.clone();
        let mut state = start;
        assert_same(step(&mut state, EngineMessage::Cued), Err(Unhandled));
        assert_eq!(state, expected);
    }

    #[test]
    fn a_seek_back_during_a_running_crossfade_keeps_the_playing_track_audible() {
        let mut state = EngineState::Live(crossfading_mid_ramp());
        assert!(step(&mut state, cmd(AudioCmd::Seek(seconds(50)))).is_ok());
        assert_same(
            step(&mut state, EngineMessage::Ramped(SinkRole::Primary)),
            Err(Unhandled),
        );
        assert_eq!(state, EngineState::Live(crossfading_idle()));
    }

    #[test]
    fn a_finished_ramp_promotes_the_incoming_track() {
        let mut state = EngineState::Live(crossfading_mid_ramp());
        let effect =
            step(&mut state, EngineMessage::Ramped(SinkRole::Primary)).unwrap();
        assert_same(
            effect,
            Cmd::effect(LoopEffect::Execute(EngineEffect::Promote(
                crate::gain::Gain::UNITY,
            )))
            .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))
            .then(Cmd::message(AudioEvent::TrackChanged)),
        );
        assert_eq!(
            state,
            EngineState::Live(promoted(Crossfade::clamped(seconds(10))))
        );
    }

    #[rstest]
    #[case::outgoing(SinkRole::Outgoing)]
    #[case::incoming(SinkRole::Incoming)]
    fn a_ramped_outgoing_or_incoming_is_ignored(#[case] role: SinkRole) {
        let mut state = EngineState::Live(crossfading_mid_ramp());
        assert_same(
            step(&mut state, EngineMessage::Ramped(role)),
            Err(Unhandled),
        );
    }

    #[rstest]
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
    #[case::ramped_primary_in_handover_ignored(
        EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Ramped(SinkRole::Primary),
        EngineRow {
            next: EngineState::Live(handing_over(Incoming::Loading(loading_track("/b")))),
            effect: Err(Unhandled),
        }
    )]
    fn a_handover_follows_its_ramps(
        #[case] start: EngineState,
        #[case] message: EngineMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
    }

    #[rstest]
    #[case::preloaded_opens_the_crossfade(
        EngineState::Live(awaiting(
            playing_with_crossfade(),
            "/b",
        )),
        installed(&track_b(), Revision::default()),
        EngineRow {
            next: EngineState::Live(crossfading_idle()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(Some(seconds(90)))))),
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
    #[case::an_install_nobody_awaits_is_ignored(
        EngineState::Live(playing()),
        installed(&track_b(), Revision::default()),
        EngineRow { next: EngineState::Live(playing()), effect: Err(Unhandled)}
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: EngineState,
        #[case] message: EngineMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
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
        assert_same(
            step(&mut state, EngineMessage::Finished(SinkRole::Incoming)),
            Err(Unhandled),
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_preload_install_after_a_stop_is_ignored() {
        let (mut state, log) = trace(
            EngineState::Live(playing_with_crossfade()),
            vec![preload("/b"), cmd(AudioCmd::Stop)],
        )
        .unwrap();
        assert_same(
            step(&mut state, installed(&track_b(), first())),
            Err(Unhandled),
        );
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert_eq!(live.phase, Phase::Idle);
    }

    #[test]
    fn an_attached_after_the_engine_moved_on_is_refused() {
        let mut engine = Engine {
            state: EngineState::Live(playing()),
            job_revisions: JobRevisions::default(),
        };
        assert!(engine.transition(preload("/b")).is_ok());
        assert!(engine.transition(preload_at("/c", second())).is_ok());
        let attached = |revision| EngineMessage::Attached {
            revision,
            preload_mode: PreloadMode::Gapless,
            duration: None,
        };
        assert_same(engine.transition(attached(first())), Err(Unhandled));
        assert_eq!(
            engine.state,
            EngineState::Live(awaiting(preloaded_at(playing(), second()), "/c"))
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
                EngineMessage::Finished(SinkRole::Primary),
            ],
        )
        .unwrap();
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_gapless_advance_applies_the_next_track_gain() {
        let gain = Some(Decibels(-6.0));
        let start = Live {
            settings: AudioSettings {
                replay_gain: ReplayGain::On,
                ..settings()
            },
            ..playing()
        };
        let (state, log) = trace(
            EngineState::Live(start),
            vec![
                cmd(AudioCmd::Preload(TrackLoad {
                    path: "/b".into(),
                    gain,
                    revision: first(),
                })),
                EngineMessage::Attached {
                    revision: first(),
                    preload_mode: PreloadMode::Gapless,
                    duration: None,
                },
                EngineMessage::Finished(SinkRole::Primary),
            ],
        )
        .unwrap();
        let advanced = Cmd::effect(LoopEffect::Execute(EngineEffect::Advance(
            replay_gain_factor(ReplayGain::On, gain),
        )))
        .then(Cmd::message(AudioEvent::TrackChanged))
        .then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)));
        assert_same(log.last(), Some(&advanced));
        assert_eq!(still_live(state).phase.current().and_then(|c| c.gain), gain);
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
                    duration: Some(PRELOAD_TOTAL),
                },
                EngineMessage::Finished(SinkRole::Primary),
                set_crossfade(CROSSFADE_SECONDS),
                preload_at("/c", second()),
                installed(&track_b(), second()),
            ],
        )
        .unwrap();
        assert_same(
            log.last(),
            Some(&Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(Some(
                PRELOAD_TOTAL - seconds(CROSSFADE_SECONDS),
            ))))),
        );
    }

    #[test]
    fn a_landed_crossfade_preload_still_hands_over() {
        let (state, log) = trace(
            EngineState::Live(playing_with_crossfade()),
            vec![preload("/b"), installed(&track_b(), first())],
        )
        .unwrap();
        let live = still_live(state);
        assert!(matches!(
            live.phase,
            Phase::Playing(Playing {
                next: Next::Crossfading { .. },
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
                    next: Next::Gapless(CurrentTrack {
                        path: requested.into(),
                        gain: None,
                        total: None,
                    }),
                    ..Playing::new(track_a())
                })
            );
        }
    }
}
