use kernel::{AudioError, AudioEvent, Cmd, update::Unhandled};

use crate::engine::{
    crossfade::{arm_cue, replay_gain_factor},
    effect::{EngineEffect, PreloadKind, SinkRole},
    phase::{CurrentTrack, Fade, Handover, Incoming, Next, Phase, Playing},
    state::{Live, then_report},
};

impl Live {
    pub(crate) fn finished(
        &mut self,
        role: SinkRole,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        self.settled(role, Live::finished_primary)
    }

    pub(crate) fn ramped(
        &mut self,
        role: SinkRole,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        self.settled(role, Live::ramped_primary)
    }

    fn settled(
        &mut self,
        role: SinkRole,
        primary: fn(&mut Live) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled>,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match role {
            SinkRole::Primary => primary(self),
            SinkRole::Outgoing => self.handover_settled(),
            SinkRole::Incoming => Err(Unhandled),
        }
    }

    fn finished_primary(&mut self) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        match std::mem::replace(&mut playing.next, Next::None) {
            Next::Gapless(path) => {
                playing.current = CurrentTrack {
                    path,
                    gain: None,
                    total: None,
                };
                Ok(then_report(
                    Cmd::effect(EngineEffect::Advance)
                        .then(Cmd::message(AudioEvent::TrackChanged)),
                ))
            }
            Next::None | Next::Preloading(_) => {
                self.phase = Phase::Idle;
                Ok(then_report(Cmd::message(AudioEvent::Ended)))
            }
            Next::Crossfading { preload, .. } => {
                playing.current = preload;
                Ok(self.promoted())
            }
        }
    }

    pub(crate) fn cued(&mut self) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
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
        Ok(Cmd::effect(EngineEffect::Crossfade {
            length: self.settings.crossfade.get(),
            incoming: replay_gain_factor(self.settings.replay_gain, preload.gain),
        }))
    }

    fn ramped_primary(&mut self) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        if !playing.promote() {
            return Err(Unhandled);
        }
        Ok(self.promoted())
    }

    fn handover_settled(&mut self) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match &self.phase {
            Phase::Handover(Handover {
                incoming: Incoming::Playing(current),
            }) => {
                self.phase = Phase::Playing(Playing::new(current.clone()));
                Ok(Cmd::effect(EngineEffect::DropOutgoing))
            }
            Phase::Idle
            | Phase::Loading(_)
            | Phase::Playing(_)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(_),
            }) => Err(Unhandled),
        }
    }

    pub(crate) fn preloaded(
        &mut self,
        preload: PreloadKind,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        let Next::Preloading(wanted) = &playing.next else {
            return Err(Unhandled);
        };
        let (next, cmd) = match preload {
            PreloadKind::Gapless(path) if path == *wanted => {
                (Next::Gapless(path), Cmd::none())
            }
            PreloadKind::Crossfade(preload) if preload.path == *wanted => {
                let cue = arm_cue(playing.current.total, self.settings.crossfade.get());
                let armed = cue.map_or_else(Cmd::none, |cue| {
                    Cmd::effect(EngineEffect::Arm(Some(cue)))
                });
                let fade = Fade::Armed;
                (Next::Crossfading { preload, fade }, armed)
            }
            PreloadKind::Gapless(_) | PreloadKind::Crossfade(_) => {
                (Next::None, Cmd::none())
            }
        };
        playing.next = next;
        Ok(cmd)
    }

    pub(crate) fn preload_failed(
        &mut self,
        error: AudioError,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        let Phase::Playing(Playing {
            next: next @ Next::Preloading(_),
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
        AudioCmd,
        AudioEvent,
        Bounded,
        Cmd,
        Playback,
        domain::Crossfade,
        update::{Machine, Unhandled},
    };
    use proptest::prelude::{prop_assert, prop_assert_eq, prop_assume, proptest};
    use rstest::rstest;

    use crate::engine::{
        effect::{EngineEffect, EngineMessage, PreloadKind, SinkRole},
        phase::{CurrentTrack, Incoming, Next, Phase, Playing},
        state::{Engine, Live},
        tests::{
            EngineRow,
            assert_cell,
            awaiting,
            closed,
            cmd,
            crossfade_preload,
            crossfading_idle,
            crossfading_mid_ramp,
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
            preload_error,
            promoted,
            seconds,
            trace,
            track_a,
            track_b,
            unhandled,
        },
    };

    struct DeckEventRow {
        start: Engine,
        expected: Engine,
        effect: Cmd<EngineEffect, AudioEvent>,
    }

    fn gapless_queued() -> Engine {
        Engine::Live(Live {
            phase: Phase::Playing(Playing {
                next: Next::Gapless("/b".into()),
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
                expected: Engine::Live(Live {
                    phase: Phase::Playing(Playing::new(CurrentTrack {
                        path: "/b".into(),
                        gain: None,
                        total: None,
                    })),
                    ..live()
                }),
                effect: Cmd::effect(EngineEffect::Advance)
                    .then(Cmd::message(AudioEvent::TrackChanged))
                    .then(Cmd::effect(EngineEffect::Report)),
            },
            DeckEventRow {
                start: Engine::Live(playing()),
                expected: Engine::Live(live()),
                effect: Cmd::message(AudioEvent::Ended)
                    .then(Cmd::effect(EngineEffect::Report)),
            },
        ];
        for row in rows {
            let mut state = row.start;
            let effect = state
                .transition(EngineMessage::Finished(SinkRole::Primary))
                .unwrap();
            assert_eq!(effect, row.effect);
            assert_eq!(state, row.expected);
        }
    }

    #[rstest]
    #[case::idle(Engine::Live(live()))]
    #[case::loading(Engine::Live(loading()))]
    #[case::closed(closed())]
    fn a_finished_outside_a_track_is_ignored(#[case] start: Engine) {
        let expected = start.clone();
        let mut state = start;
        assert_eq!(
            state.transition(EngineMessage::Finished(SinkRole::Primary)),
            Err(Unhandled)
        );
        assert_eq!(state, expected);
    }

    #[rstest]
    #[case::outgoing(SinkRole::Outgoing)]
    #[case::incoming(SinkRole::Incoming)]
    fn a_finished_outgoing_or_incoming_without_a_handover_is_nothing(
        #[case] role: SinkRole,
    ) {
        let mut state = Engine::Live(playing());
        assert_eq!(
            state.transition(EngineMessage::Finished(role)),
            Err(Unhandled)
        );
    }

    #[test]
    fn a_finish_mid_crossfade_promotes() {
        let mut state = Engine::Live(crossfading_mid_ramp());
        let effect = state
            .transition(EngineMessage::Finished(SinkRole::Primary))
            .unwrap();
        assert_eq!(
            effect,
            Cmd::effect(EngineEffect::Promote(crate::gain::Gain::UNITY))
                .then(Cmd::effect(EngineEffect::Report))
                .then(Cmd::message(AudioEvent::TrackChanged))
        );
        assert_eq!(
            state,
            Engine::Live(promoted(Crossfade::clamped(seconds(10))))
        );
    }

    #[test]
    fn a_cued_track_fades_in_and_out() {
        let mut state = Engine::Live(crossfading_idle());
        let effect = state.transition(EngineMessage::Cued).unwrap();
        assert_eq!(
            effect,
            Cmd::effect(EngineEffect::Crossfade {
                length: seconds(10),
                incoming: crate::gain::Gain::UNITY,
            })
        );
        assert_eq!(state, Engine::Live(crossfading_mid_ramp()));
    }

    #[test]
    fn a_second_cue_is_ignored() {
        let mut state = Engine::Live(crossfading_mid_ramp());
        assert_eq!(state.transition(EngineMessage::Cued), Err(Unhandled));
        assert_eq!(state, Engine::Live(crossfading_mid_ramp()));
    }

    #[rstest]
    #[case::idle(Engine::Live(live()))]
    #[case::loading(Engine::Live(loading()))]
    fn a_cue_while_idle_or_loading_is_ignored(#[case] start: Engine) {
        let expected = start.clone();
        let mut state = start;
        assert_eq!(state.transition(EngineMessage::Cued), Err(Unhandled));
        assert_eq!(state, expected);
    }

    #[test]
    fn a_finished_ramp_promotes_the_incoming_track() {
        let mut state = Engine::Live(crossfading_mid_ramp());
        let effect = state
            .transition(EngineMessage::Ramped(SinkRole::Primary))
            .unwrap();
        assert_eq!(
            effect,
            Cmd::effect(EngineEffect::Promote(crate::gain::Gain::UNITY))
                .then(Cmd::effect(EngineEffect::Report))
                .then(Cmd::message(AudioEvent::TrackChanged))
        );
        assert_eq!(
            state,
            Engine::Live(promoted(Crossfade::clamped(seconds(10))))
        );
    }

    #[rstest]
    #[case::outgoing(SinkRole::Outgoing)]
    #[case::incoming(SinkRole::Incoming)]
    fn a_ramped_outgoing_or_incoming_is_ignored(#[case] role: SinkRole) {
        let mut state = Engine::Live(crossfading_mid_ramp());
        assert_eq!(
            state.transition(EngineMessage::Ramped(role)),
            Err(Unhandled)
        );
    }

    #[rstest]
    #[case::ramped_outgoing_drops_it(
        Engine::Live(handed_over_to_b()),
        EngineMessage::Ramped(SinkRole::Outgoing),
        EngineRow {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..handed_over_to_b() }),
            effect: Cmd::effect(EngineEffect::DropOutgoing),
        }
    )]
    #[case::finished_outgoing_drops_it(
        Engine::Live(handed_over_to_b()),
        EngineMessage::Finished(SinkRole::Outgoing),
        EngineRow {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..handed_over_to_b() }),
            effect: Cmd::effect(EngineEffect::DropOutgoing),
        }
    )]
    #[case::ramped_primary_in_handover_ignored(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Ramped(SinkRole::Primary),
        EngineRow {
            next: Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
            effect: Cmd::none(),
        }
    )]
    fn a_handover_follows_its_ramps(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
    }

    #[rstest]
    #[case::preloaded_opens_the_crossfade(
        Engine::Live(awaiting(
            playing_with_crossfade(),
            "/b",
        )),
        installed(track_b()),
        EngineRow {
            next: Engine::Live(crossfading_idle()),
            effect: Cmd::effect(EngineEffect::Arm(Some(seconds(90)))),
        }
    )]
    #[case::a_failed_preload_is_reported(
        Engine::Live(awaiting(playing(), "/b")),
        EngineMessage::Error(preload_error()),
        EngineRow {
            next: Engine::Live(playing()),
            effect: Cmd::message(AudioEvent::Error(preload_error())),
        }
    )]
    #[case::an_install_nobody_awaits_is_ignored(
        Engine::Live(playing()),
        installed(track_b()),
        EngineRow { next: Engine::Live(playing()), effect: Cmd::none()}
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
    }

    fn still_live(state: Engine) -> Live {
        match state {
            Engine::Live(live) => live,
            Engine::Closed(_) => panic!("the engine must stay live"),
        }
    }

    #[test]
    fn a_preload_still_decoding_holds_up_neither_a_pause_nor_a_deck_event() {
        let (mut state, log) = trace(
            Engine::Live(playing()),
            vec![preload("/b"), cmd(AudioCmd::Playback(Playback::Paused))],
        )
        .unwrap();
        assert_eq!(
            state.transition(EngineMessage::Finished(SinkRole::Incoming)),
            Err(Unhandled)
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_preload_install_after_a_stop_is_ignored() {
        let (mut state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/b"), cmd(AudioCmd::Stop)],
        )
        .unwrap();
        assert_eq!(state.transition(installed(track_b())), Err(Unhandled));
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert_eq!(live.phase, Phase::Idle);
    }

    #[test]
    fn a_preload_install_for_a_superseded_track_is_ignored() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/c"), installed(track_b())],
        )
        .unwrap();
        assert_eq!(log, vec![crossfade_preload("/c"), Cmd::none()]);

        let live = still_live(state);
        assert_eq!(live.phase, playing_with_crossfade().phase);
    }

    #[test]
    fn a_landed_gapless_preload_still_hands_over_at_the_end() {
        let (_, log) = trace(
            Engine::Live(playing()),
            vec![
                preload("/b"),
                EngineMessage::Preloaded(PreloadKind::Gapless("/b".into())),
                EngineMessage::Finished(SinkRole::Primary),
            ],
        )
        .unwrap();
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_landed_crossfade_preload_still_hands_over() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/b"), installed(track_b())],
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
        fn a_preload_result_for_a_different_path_than_requested_is_superseded(
            requested in "[a-z]{1,8}",
            landed_path in "[a-z]{1,8}",
        ) {
            prop_assume!(requested != landed_path);
            let mut state = Engine::Live(playing());
            prop_assert!(state.transition(preload(&requested)).is_ok());
            let effect = state
                .transition(EngineMessage::Preloaded(PreloadKind::Crossfade(CurrentTrack {
                    path: landed_path.into(),
                    gain: None,
                    total: None,
                })))
                .map_err(unhandled)?;
            prop_assert_eq!(effect, Cmd::none());
            let Engine::Live(live) = state else {
                return Err(unhandled("the engine stays live across a preload"));
            };
            prop_assert_eq!(live.phase, playing().phase);
        }
    }
}
