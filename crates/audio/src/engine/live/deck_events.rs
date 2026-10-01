use std::path::PathBuf;

use kernel::{AudioError, AudioEvent};

use crate::engine::{
    crossfade::{arm_cue, replaygain_factor},
    effect::{EngineEffect, Preload, SinkRole},
    phase::{CurrentTrack, Handover, Incoming, Next, Phase, Playing},
    state::{Engine, Live, then_report},
};

impl Live {
    pub(crate) fn finished(self, role: SinkRole) -> (Engine, EngineEffect) {
        self.settled(role, Live::finished_primary)
    }

    pub(crate) fn ramped(self, role: SinkRole) -> (Engine, EngineEffect) {
        self.settled(role, Live::ramped_primary)
    }

    fn settled(
        self,
        role: SinkRole,
        primary: fn(Live) -> (Engine, EngineEffect),
    ) -> (Engine, EngineEffect) {
        match role {
            SinkRole::Primary => primary(self),
            SinkRole::Outgoing => self.handover_settled(),
            SinkRole::Incoming => (Engine::Live(self), EngineEffect::Nothing),
        }
    }

    fn finished_primary(mut self) -> (Engine, EngineEffect) {
        let Some(playing) = self.take_playing() else {
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        match playing.next {
            Next::Gapless(path) => self.finished_gapless(path, playing.preloading),
            Next::None => self.finished_idle(),
            Next::Crossfading { preload, .. } => {
                self.promote(preload, playing.preloading)
            }
        }
    }

    fn finished_gapless(
        mut self,
        path: PathBuf,
        preloading: Option<PathBuf>,
    ) -> (Engine, EngineEffect) {
        self.phase = Phase::Playing(Playing {
            current: CurrentTrack {
                path,
                gain: None,
                total: None,
            },
            next: Next::None,
            preloading,
        });
        let effect = EngineEffect::Batch(vec![
            EngineEffect::Advance,
            EngineEffect::Send(AudioEvent::TrackChanged),
        ]);
        (Engine::Live(self), then_report(effect))
    }

    fn finished_idle(mut self) -> (Engine, EngineEffect) {
        self.phase = Phase::Idle;
        (
            Engine::Live(self),
            then_report(EngineEffect::Send(AudioEvent::Ended)),
        )
    }

    pub(crate) fn cued(mut self) -> (Engine, EngineEffect) {
        let Some(mut playing) = self.take_playing() else {
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        let taken = std::mem::take(&mut playing.next);
        let Next::Crossfading {
            preload,
            fading: false,
        } = taken
        else {
            playing.next = taken;
            self.phase = Phase::Playing(playing);
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        let incoming = replaygain_factor(self.config.replaygain, preload.gain);
        let length = self.config.crossfade.get();
        playing.next = Next::Crossfading {
            preload,
            fading: true,
        };
        self.phase = Phase::Playing(playing);
        (
            Engine::Live(self),
            EngineEffect::Crossfade { length, incoming },
        )
    }

    fn ramped_primary(mut self) -> (Engine, EngineEffect) {
        let Some(playing) = self.take_playing() else {
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        match playing.next {
            Next::Crossfading { preload, .. } => {
                self.promote(preload, playing.preloading)
            }
            next @ (Next::None | Next::Gapless(_)) => {
                self.phase = Phase::Playing(Playing { next, ..playing });
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }

    fn handover_settled(mut self) -> (Engine, EngineEffect) {
        match std::mem::take(&mut self.phase) {
            Phase::Handover(Handover {
                incoming: Incoming::Playing(current),
            }) => {
                self.phase = Phase::Playing(Playing::new(current));
                (Engine::Live(self), EngineEffect::DropOutgoing)
            }
            phase @ (Phase::Idle
            | Phase::Loading(_)
            | Phase::Playing(_)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(_),
            })) => {
                self.phase = phase;
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }
}

impl Live {
    pub(crate) fn preloaded(
        mut self,
        outcome: Result<Preload, AudioError>,
    ) -> (Engine, EngineEffect) {
        let Some(mut playing) = self.take_playing() else {
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        let Some(wanted) = playing.preloading.take() else {
            self.phase = Phase::Playing(playing);
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        let effect = match outcome {
            Err(error) => EngineEffect::Send(AudioEvent::Error(error)),
            Ok(Preload::Gapless(path)) => {
                if path == wanted {
                    playing.next = Next::Gapless(path);
                }
                EngineEffect::Nothing
            }
            Ok(Preload::Crossfade(preload)) => {
                if preload.path == wanted {
                    let cue =
                        arm_cue(playing.current.total, self.config.crossfade.get());
                    playing.next = Next::Crossfading {
                        preload,
                        fading: false,
                    };
                    cue.map_or(EngineEffect::Nothing, |cue| EngineEffect::Arm {
                        cue: Some(cue),
                    })
                } else {
                    EngineEffect::Nothing
                }
            }
        };
        self.phase = Phase::Playing(playing);
        (Engine::Live(self), effect)
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        AudioCmd,
        AudioEvent,
        Bounded,
        Playback,
        domain::Crossfade,
        update::Machine,
    };
    use proptest::{
        prelude::{prop_assert_eq, prop_assume, proptest},
        test_runner::TestCaseError,
    };
    use rstest::rstest;

    use crate::engine::{
        effect::{EngineEffect, EngineMessage, Preload, SinkRole},
        phase::{CurrentTrack, Incoming, Next, Phase, Playing},
        state::{Engine, Live},
        test_support::{
            Cell,
            assert_cell,
            awaiting,
            cmd,
            crossfade_preload,
            crossfading_idle,
            crossfading_mid_ramp,
            decode_error,
            handed_over_to_b,
            handing_over,
            installed,
            live,
            loading,
            loading_track,
            muted,
            playing,
            playing_track,
            playing_with_crossfade,
            preload,
            promoted,
            seconds,
            track_a,
            track_b,
        },
    };

    struct Row {
        start: Engine,
        expected: Engine,
        effect: EngineEffect,
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
            Row {
                start: gapless_queued(),
                expected: Engine::Live(Live {
                    phase: Phase::Playing(Playing::new(CurrentTrack {
                        path: "/b".into(),
                        gain: None,
                        total: None,
                    })),
                    ..live()
                }),
                effect: EngineEffect::Batch(vec![
                    EngineEffect::Advance,
                    EngineEffect::Send(AudioEvent::TrackChanged),
                    EngineEffect::Report,
                ]),
            },
            Row {
                start: Engine::Live(playing()),
                expected: Engine::Live(live()),
                effect: EngineEffect::Batch(vec![
                    EngineEffect::Send(AudioEvent::Ended),
                    EngineEffect::Report,
                ]),
            },
        ];
        for row in rows {
            let mut state = row.start;
            let effect = state
                .update(EngineMessage::Finished(SinkRole::Primary))
                .unwrap();
            assert_eq!(effect, row.effect);
            assert_eq!(state, row.expected);
        }
    }

    #[rstest]
    #[case::idle(Engine::Live(live()))]
    #[case::loading(Engine::Live(loading()))]
    #[case::muted(muted())]
    fn a_finished_outside_a_track_is_ignored(#[case] start: Engine) {
        let expected = start.clone();
        let mut state = start;
        assert_eq!(
            state.update(EngineMessage::Finished(SinkRole::Primary)),
            Ok(EngineEffect::Nothing)
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
            state.update(EngineMessage::Finished(role)),
            Ok(EngineEffect::Nothing)
        );
    }

    #[test]
    fn a_finish_mid_crossfade_promotes() {
        let mut state = Engine::Live(crossfading_mid_ramp());
        let effect = state
            .update(EngineMessage::Finished(SinkRole::Primary))
            .unwrap();
        assert_eq!(
            effect,
            EngineEffect::Batch(vec![
                EngineEffect::Promote { volume: 1.0 },
                EngineEffect::Report,
            ])
        );
        assert_eq!(
            state,
            Engine::Live(promoted(Crossfade::clamped(seconds(10))))
        );
    }

    #[test]
    fn a_cued_track_fades_in_and_out() {
        let mut state = Engine::Live(crossfading_idle());
        let effect = state.update(EngineMessage::Cued).unwrap();
        assert_eq!(
            effect,
            EngineEffect::Crossfade {
                length: seconds(10),
                incoming: 1.0,
            }
        );
        assert_eq!(state, Engine::Live(crossfading_mid_ramp()));
    }

    #[test]
    fn a_second_cue_is_ignored() {
        let mut state = Engine::Live(crossfading_mid_ramp());
        assert_eq!(state.update(EngineMessage::Cued), Ok(EngineEffect::Nothing));
        assert_eq!(state, Engine::Live(crossfading_mid_ramp()));
    }

    #[rstest]
    #[case::idle(Engine::Live(live()))]
    #[case::loading(Engine::Live(loading()))]
    fn a_cue_while_idle_or_loading_is_ignored(#[case] start: Engine) {
        let expected = start.clone();
        let mut state = start;
        assert_eq!(state.update(EngineMessage::Cued), Ok(EngineEffect::Nothing));
        assert_eq!(state, expected);
    }

    #[test]
    fn a_finished_ramp_promotes_the_incoming_track() {
        let mut state = Engine::Live(crossfading_mid_ramp());
        let effect = state
            .update(EngineMessage::Ramped(SinkRole::Primary))
            .unwrap();
        assert_eq!(
            effect,
            EngineEffect::Batch(vec![
                EngineEffect::Promote { volume: 1.0 },
                EngineEffect::Report,
            ])
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
            state.update(EngineMessage::Ramped(role)),
            Ok(EngineEffect::Nothing)
        );
    }

    #[rstest]
    #[case::ramped_outgoing_drops_it(
        Engine::Live(handed_over_to_b()),
        EngineMessage::Ramped(SinkRole::Outgoing),
        Cell {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..handed_over_to_b() }),
            effect: EngineEffect::DropOutgoing,
        }
    )]
    #[case::finished_outgoing_drops_it(
        Engine::Live(handed_over_to_b()),
        EngineMessage::Finished(SinkRole::Outgoing),
        Cell {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..handed_over_to_b() }),
            effect: EngineEffect::DropOutgoing,
        }
    )]
    #[case::ramped_primary_in_handover_ignored(
        Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
        EngineMessage::Ramped(SinkRole::Primary),
        Cell {
            next: Engine::Live(handing_over(Incoming::Loading(loading_track("/b")))),
            effect: EngineEffect::Nothing,
        }
    )]
    fn a_handover_follows_its_ramps(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Cell,
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
        Cell {
            next: Engine::Live(crossfading_idle()),
            effect: EngineEffect::Arm { cue: Some(seconds(90)) },
        }
    )]
    #[case::a_failed_preload_is_reported(
        Engine::Live(awaiting(playing(), "/b")),
        EngineMessage::Preloaded(Err(decode_error())),
        Cell {
            next: Engine::Live(playing()),
            effect: EngineEffect::Send(AudioEvent::Error(decode_error())),
        }
    )]
    #[case::an_install_nobody_awaits_is_ignored(
        Engine::Live(playing()),
        installed(track_b()),
        Cell { next: Engine::Live(playing()), effect: EngineEffect::Nothing }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Cell,
    ) {
        assert_cell(start, message, moved);
    }

    fn still_live(state: Engine) -> Live {
        match state {
            Engine::Live(live) => live,
            Engine::Muted(_) => panic!("the engine must stay live"),
        }
    }

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

    #[test]
    fn a_preload_still_decoding_holds_up_neither_a_pause_nor_a_deck_event() {
        let (_, log) = trace(
            Engine::Live(playing()),
            vec![
                preload("/b"),
                cmd(AudioCmd::Playback(Playback::Paused)),
                EngineMessage::Finished(SinkRole::Incoming),
            ],
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_preload_install_after_a_stop_is_ignored() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/b"), cmd(AudioCmd::Stop), installed(track_b())],
        );
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert_eq!(live.phase, Phase::Idle);
    }

    #[test]
    fn a_preload_install_for_a_superseded_track_is_ignored() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/c"), installed(track_b())],
        );
        assert_eq!(log, vec![crossfade_preload("/c"), EngineEffect::Nothing,]);

        let live = still_live(state);
        assert_eq!(live.phase, playing_with_crossfade().phase);
    }

    #[test]
    fn a_landed_gapless_preload_still_hands_over_at_the_end() {
        let (_, log) = trace(
            Engine::Live(playing()),
            vec![
                preload("/b"),
                EngineMessage::Preloaded(Ok(Preload::Gapless("/b".into()))),
                EngineMessage::Finished(SinkRole::Primary),
            ],
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_landed_crossfade_preload_still_hands_over() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/b"), installed(track_b())],
        );
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

    fn rejected(reason: impl std::fmt::Debug) -> TestCaseError {
        TestCaseError::fail(format!(
            "the machine rejected a valid transition: {reason:?}"
        ))
    }

    proptest! {
        #[test]
        fn a_preload_result_for_a_different_path_than_requested_is_superseded(
            requested in "[a-z]{1,8}",
            landed_path in "[a-z]{1,8}",
        ) {
            prop_assume!(requested != landed_path);
            let mut state = Engine::Live(playing());
            state.update(preload(&requested)).map_err(rejected)?;
            let effect = state
                .update(EngineMessage::Preloaded(Ok(Preload::Crossfade(CurrentTrack {
                    path: landed_path.into(),
                    gain: None,
                    total: None,
                }))))
                .map_err(rejected)?;
            prop_assert_eq!(effect, EngineEffect::Nothing);
            let Engine::Live(live) = state else {
                return Err(rejected("the engine stays live across a preload"));
            };
            prop_assert_eq!(live.phase, playing().phase);
        }
    }
}
