use std::path::PathBuf;

use kernel::AudioEvent;

use crate::engine::{
    crossfade::effective_volume,
    effect::{EngineEffect, Slot},
    phase::{CurrentTrack, Fade, Handover, Incoming, Next, Outgoing, Phase, Playing},
    state::{Engine, Live, reported},
};

impl Live {
    pub(crate) fn finished(self, slot: Slot) -> (Engine, EngineEffect) {
        match slot {
            Slot::Primary => self.finished_primary(),
            Slot::Outgoing => self.handover_settled(),
            Slot::Incoming => (Engine::Live(self), EngineEffect::Nothing),
        }
    }

    fn finished_primary(mut self) -> (Engine, EngineEffect) {
        let playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return (Engine::Live(self), EngineEffect::Nothing);
            }
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
        let io = EngineEffect::Many(vec![
            EngineEffect::Advance,
            EngineEffect::Send(AudioEvent::TrackChanged),
        ]);
        (Engine::Live(self), reported(io))
    }

    fn finished_idle(mut self) -> (Engine, EngineEffect) {
        self.phase = Phase::Idle;
        (
            Engine::Live(self),
            reported(EngineEffect::Send(AudioEvent::Ended)),
        )
    }

    pub(crate) fn cued(mut self) -> (Engine, EngineEffect) {
        let mut playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return (Engine::Live(self), EngineEffect::Nothing);
            }
        };
        let taken = std::mem::take(&mut playing.next);
        let Next::Crossfading {
            preload,
            fade: Fade::Idle,
        } = taken
        else {
            playing.next = taken;
            self.phase = Phase::Playing(playing);
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        let incoming =
            effective_volume(&self.config, preload.gain, self.user_factor.ratio());
        let length = self.config.crossfade.value();
        playing.next = Next::Crossfading {
            preload,
            fade: Fade::Fading,
        };
        self.phase = Phase::Playing(playing);
        (
            Engine::Live(self),
            EngineEffect::Crossfade { length, incoming },
        )
    }

    pub(crate) fn ramped(self, slot: Slot) -> (Engine, EngineEffect) {
        match slot {
            Slot::Primary => self.ramped_primary(),
            Slot::Outgoing => self.handover_settled(),
            Slot::Incoming => (Engine::Live(self), EngineEffect::Nothing),
        }
    }

    fn ramped_primary(mut self) -> (Engine, EngineEffect) {
        let playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return (Engine::Live(self), EngineEffect::Nothing);
            }
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
                ..
            }) => {
                self.phase = Phase::Playing(Playing::new(current));
                (Engine::Live(self), EngineEffect::DropOutgoing)
            }
            phase @ (Phase::Idle
            | Phase::Loading(_)
            | Phase::Playing(_)
            | Phase::Handover(Handover {
                incoming: Incoming::Loading(_),
                ..
            })) => {
                self.phase = phase;
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }

    pub(crate) fn retiring(mut self, from: f32) -> (Engine, EngineEffect) {
        match std::mem::take(&mut self.phase) {
            Phase::Handover(Handover { incoming, .. }) => {
                self.phase = Phase::Handover(Handover {
                    outgoing: Outgoing { from },
                    incoming,
                });
                (Engine::Live(self), EngineEffect::Nothing)
            }
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Playing(_)) => {
                self.phase = phase;
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, domain::Crossfade, update::Machine};
    use rstest::rstest;

    use crate::engine::{
        effect::{EngineEffect, EngineMessage, Slot},
        phase::{CurrentTrack, Fade, Incoming, Next, Outgoing, Phase, Playing},
        state::{
            Engine,
            Live,
            fixtures::{
                crossfading,
                handing_over,
                live,
                loading,
                loading_track,
                muted,
                playing,
                playing_track,
                promoted,
                retiring,
                secs,
                track_a,
                track_b,
            },
        },
    };

    struct Row {
        start: Engine,
        expected: Engine,
        io: EngineEffect,
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
                io: EngineEffect::Many(vec![
                    EngineEffect::Advance,
                    EngineEffect::Send(kernel::AudioEvent::TrackChanged),
                    EngineEffect::Report,
                ]),
            },
            Row {
                start: Engine::Live(playing()),
                expected: Engine::Live(live()),
                io: EngineEffect::Many(vec![
                    EngineEffect::Send(kernel::AudioEvent::Ended),
                    EngineEffect::Report,
                ]),
            },
        ];
        for row in rows {
            let mut state = row.start;
            let effect = state
                .update(EngineMessage::Finished(Slot::Primary))
                .unwrap();
            assert_eq!(effect, row.io);
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
            state.update(EngineMessage::Finished(Slot::Primary)),
            Ok(EngineEffect::Nothing)
        );
        assert_eq!(state, expected);
    }

    #[rstest]
    #[case::outgoing(Slot::Outgoing)]
    #[case::incoming(Slot::Incoming)]
    fn a_finished_outgoing_or_incoming_without_a_handover_is_nothing(
        #[case] slot: Slot,
    ) {
        let mut state = Engine::Live(playing());
        assert_eq!(
            state.update(EngineMessage::Finished(slot)),
            Ok(EngineEffect::Nothing)
        );
    }

    #[test]
    fn finished_while_fading_promotes() {
        let mut state = Engine::Live(crossfading(Fade::Fading));
        let effect = state
            .update(EngineMessage::Finished(Slot::Primary))
            .unwrap();
        assert_eq!(
            effect,
            EngineEffect::Many(vec![
                EngineEffect::Promote { volume: 1.0 },
                EngineEffect::Report,
            ])
        );
        assert_eq!(state, Engine::Live(promoted(Crossfade::clamped(secs(10)))));
    }

    #[test]
    fn cued_fades() {
        let mut state = Engine::Live(crossfading(Fade::Idle));
        let effect = state.update(EngineMessage::Cued).unwrap();
        assert_eq!(
            effect,
            EngineEffect::Crossfade {
                length: secs(10),
                incoming: 1.0,
            }
        );
        assert_eq!(state, Engine::Live(crossfading(Fade::Fading)));
    }

    #[test]
    fn cued_twice_ignored() {
        let mut state = Engine::Live(crossfading(Fade::Fading));
        assert_eq!(state.update(EngineMessage::Cued), Ok(EngineEffect::Nothing));
        assert_eq!(state, Engine::Live(crossfading(Fade::Fading)));
    }

    #[rstest]
    #[case::idle(Engine::Live(live()))]
    #[case::loading(Engine::Live(loading()))]
    fn cued_while_loading_ignored(#[case] start: Engine) {
        let expected = start.clone();
        let mut state = start;
        assert_eq!(state.update(EngineMessage::Cued), Ok(EngineEffect::Nothing));
        assert_eq!(state, expected);
    }

    #[test]
    fn ramped_promotes() {
        let mut state = Engine::Live(crossfading(Fade::Fading));
        let effect = state.update(EngineMessage::Ramped(Slot::Primary)).unwrap();
        assert_eq!(
            effect,
            EngineEffect::Many(vec![
                EngineEffect::Promote { volume: 1.0 },
                EngineEffect::Report,
            ])
        );
        assert_eq!(state, Engine::Live(promoted(Crossfade::clamped(secs(10)))));
    }

    #[rstest]
    #[case::outgoing(Slot::Outgoing)]
    #[case::incoming(Slot::Incoming)]
    fn a_ramped_outgoing_or_incoming_is_ignored(#[case] slot: Slot) {
        let mut state = Engine::Live(crossfading(Fade::Fading));
        assert_eq!(
            state.update(EngineMessage::Ramped(slot)),
            Ok(EngineEffect::Nothing)
        );
    }

    struct Transition {
        next: Engine,
        io: EngineEffect,
    }

    #[rstest]
    #[case::retiring_records_from(
        Engine::Live(handing_over(
            Outgoing { from: 0.0 },
            Incoming::Loading(loading_track("/b")),
        )),
        EngineMessage::Retiring { from: 0.8 },
        Transition {
            next: Engine::Live(handing_over(
                Outgoing { from: 0.8 },
                Incoming::Loading(loading_track("/b")),
            )),
            io: EngineEffect::Nothing,
        }
    )]
    #[case::retiring_outside_a_handover_is_ignored(
        Engine::Live(playing()),
        EngineMessage::Retiring { from: 0.8 },
        Transition { next: Engine::Live(playing()), io: EngineEffect::Nothing }
    )]
    #[case::ramped_outgoing_drops_it(
        Engine::Live(retiring(0.8)),
        EngineMessage::Ramped(Slot::Outgoing),
        Transition {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..retiring(0.8) }),
            io: EngineEffect::DropOutgoing,
        }
    )]
    #[case::finished_outgoing_drops_it(
        Engine::Live(retiring(0.8)),
        EngineMessage::Finished(Slot::Outgoing),
        Transition {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..retiring(0.8) }),
            io: EngineEffect::DropOutgoing,
        }
    )]
    #[case::ramped_primary_in_handover_ignored(
        Engine::Live(handing_over(
            Outgoing { from: 0.8 },
            Incoming::Loading(loading_track("/b")),
        )),
        EngineMessage::Ramped(Slot::Primary),
        Transition {
            next: Engine::Live(handing_over(
                Outgoing { from: 0.8 },
                Incoming::Loading(loading_track("/b")),
            )),
            io: EngineEffect::Nothing,
        }
    )]
    fn a_handover_follows_its_ramps(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Transition,
    ) {
        let mut state = start;
        let effect = state.update(message).unwrap();
        assert_eq!(state, moved.next);
        assert_eq!(effect, moved.io);
    }
}
