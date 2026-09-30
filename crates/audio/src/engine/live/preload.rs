use kernel::{AudioError, AudioEvent};

use crate::engine::{
    crossfade::arm_cue,
    effect::{EngineEffect, Preload},
    phase::{Fade, Next, Phase},
    state::{Engine, Live},
};

impl Live {
    pub(crate) fn preloaded(
        mut self,
        outcome: Result<Preload, AudioError>,
    ) -> (Engine, EngineEffect) {
        let mut playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return (Engine::Live(self), EngineEffect::Nothing);
            }
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
                        arm_cue(playing.current.total, self.config.crossfade.value());
                    playing.next = Next::Crossfading {
                        preload,
                        fade: Fade::Idle,
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
    use kernel::{AudioCmd, AudioEvent, Playback, domain::Speed, update::Machine};
    use proptest::{
        prelude::{prop_assert_eq, prop_assume, proptest},
        test_runner::TestCaseError,
    };
    use rstest::rstest;

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage, Preload, PreloadedTrack, Slot},
            phase::{Fade, Next, Phase, Playing},
            state::{
                Engine,
                Live,
                fixtures::{
                    awaiting,
                    cmd,
                    config,
                    crossfade,
                    crossfading,
                    decode_error,
                    installed,
                    playing,
                    playing_with_crossfade,
                    preload,
                    preload_b,
                    seconds,
                },
            },
        },
    };

    struct Cell {
        next: Engine,
        effect: EngineEffect,
    }

    #[rstest]
    #[case::preloaded_opens_the_crossfade(
        Engine::Live(awaiting(
            Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..playing() },
            "/b",
        )),
        installed(preload_b()),
        Cell {
            next: Engine::Live(crossfading(Fade::Idle)),
            effect: EngineEffect::Arm { cue: Some(seconds(90)) },
        }
    )]
    #[case::preload_failure_is_reported(
        Engine::Live(awaiting(playing(), "/b")),
        EngineMessage::Preloaded(Err(decode_error())),
        Cell {
            next: Engine::Live(playing()),
            effect: EngineEffect::Send(AudioEvent::Error(decode_error())),
        }
    )]
    #[case::a_preload_install_after_a_stop_is_dropped(
        Engine::Live(playing()),
        installed(preload_b()),
        Cell { next: Engine::Live(playing()), effect: EngineEffect::Nothing }
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
                EngineMessage::Finished(Slot::Incoming),
            ],
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_preload_install_after_a_stop_is_ignored() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/b"), cmd(AudioCmd::Stop), installed(preload_b())],
        );
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert_eq!(live.phase, Phase::Idle);
    }

    #[test]
    fn a_preload_install_for_a_superseded_track_is_ignored() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/c"), installed(preload_b())],
        );
        assert_eq!(
            log,
            vec![
                EngineEffect::PreloadCrossfade {
                    path: "/c".into(),
                    gain: None,
                    speed: Speed::default()
                },
                EngineEffect::Nothing,
            ]
        );

        let live = still_live(state);
        assert_eq!(live.phase, playing_with_crossfade().phase);
    }

    #[test]
    fn a_landed_gapless_preload_still_hands_off_at_the_end() {
        let (_, log) = trace(
            Engine::Live(playing()),
            vec![
                preload("/b"),
                EngineMessage::Preloaded(Ok(Preload::Gapless("/b".into()))),
                EngineMessage::Finished(Slot::Primary),
            ],
        );
        insta::assert_debug_snapshot!(log);
    }

    #[test]
    fn a_landed_crossfade_preload_still_hands_off() {
        let (state, log) = trace(
            Engine::Live(playing_with_crossfade()),
            vec![preload("/b"), installed(preload_b())],
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
                .update(EngineMessage::Preloaded(Ok(Preload::Crossfade(PreloadedTrack {
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
