use std::time::Duration;

use kernel::AudioEvent;

use crate::engine::{
    crossfade::{
        CrossfadeAction,
        CrossfadeMoment,
        SinkDrain,
        crossfade_action,
        effective_volume,
        fade_fraction,
        gain_in,
        gain_out,
    },
    effect::{EngineEffect, PreloadedTrack},
    phase::{Fade, Handover, Incoming, Next, Outgoing, Phase, Playing},
    state::{Engine, Live},
};

#[derive(Clone, Copy)]
struct Observation {
    queue_len: usize,
    position: Duration,
}

struct Armed {
    playing: Playing,
    preload: PreloadedTrack,
}

impl Live {
    pub(crate) fn observed(
        mut self,
        queue_len: usize,
        position: Duration,
    ) -> (Engine, EngineEffect) {
        let observation = Observation {
            queue_len,
            position,
        };
        match std::mem::take(&mut self.phase) {
            Phase::Handover(handover) => self.observed_retiring(handover, observation),
            Phase::Playing(mut playing) => match std::mem::take(&mut playing.next) {
                Next::None => self.observed_gapless(playing, observation),
                Next::Crossfading { preload, .. } => {
                    self.observed_crossfading(Armed { playing, preload }, observation)
                }
            },
            phase @ (Phase::Idle | Phase::Loading(_)) => {
                self.phase = phase;
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }

    fn position_changed(&mut self, position: Duration) -> bool {
        let changed = self.last_position != Some(position);
        self.last_position = Some(position);
        changed
    }

    fn position_event(&mut self, position: Duration) -> EngineEffect {
        if self.position_changed(position) {
            EngineEffect::Send(AudioEvent::Position(position))
        } else {
            EngineEffect::Nothing
        }
    }

    fn observed_gapless(
        mut self,
        playing: Playing,
        observation: Observation,
    ) -> (Engine, EngineEffect) {
        let Observation {
            queue_len,
            position,
        } = observation;
        let io = match gapless_event(playing.previous_queue_len, queue_len, position) {
            Some(AudioEvent::Position(at)) => self.position_event(at),
            Some(event) => EngineEffect::Send(event),
            None => EngineEffect::Nothing,
        };
        self.phase = Phase::Playing(Playing {
            previous_queue_len: queue_len,
            ..playing
        });
        (Engine::Live(self), io)
    }

    fn observed_retiring(
        mut self,
        handover: Handover,
        observation: Observation,
    ) -> (Engine, EngineEffect) {
        let Observation {
            queue_len,
            position,
        } = observation;
        let Handover { outgoing, incoming } = handover;
        let current = match incoming {
            Incoming::Playing(current) => current,
            Incoming::Loading(loading) => {
                self.phase = Phase::Handover(Handover {
                    outgoing,
                    incoming: Incoming::Loading(loading),
                });
                let io = EngineEffect::Retire {
                    playing: 0.0,
                    retiring: outgoing.from,
                    at: position,
                };
                return (Engine::Live(self), io);
            }
        };
        let fraction = fade_fraction(position, self.config.crossfade.value());
        let playing =
            effective_volume(&self.config, current.gain, self.user_factor.ratio())
                * gain_in(fraction);
        if fraction >= 1.0 {
            self.phase = Phase::Playing(Playing {
                previous_queue_len: queue_len,
                ..Playing::new(current)
            });
            let io = EngineEffect::Retired {
                playing,
                at: position,
            };
            return (Engine::Live(self), io);
        }
        self.phase = Phase::Handover(Handover {
            outgoing: Outgoing {
                fraction,
                ..outgoing
            },
            incoming: Incoming::Playing(current),
        });
        let io = EngineEffect::Retire {
            playing,
            retiring: outgoing.from * gain_out(fraction),
            at: position,
        };
        (Engine::Live(self), io)
    }

    fn observed_crossfading(
        mut self,
        armed: Armed,
        observation: Observation,
    ) -> (Engine, EngineEffect) {
        let Armed { playing, preload } = armed;
        let Observation {
            queue_len,
            position,
        } = observation;
        let sink_drained = if queue_len == 0 {
            SinkDrain::Drained
        } else {
            SinkDrain::Playing
        };
        let action = crossfade_action(&CrossfadeMoment {
            sink_drained,
            total: playing.current.total,
            position,
            crossfade: self.config.crossfade.value(),
        });
        let fade = match action {
            CrossfadeAction::Handoff => {
                return self.promote(preload, playing.preloading);
            }
            CrossfadeAction::Fade(fraction) => Fade::Fading(fraction),
            CrossfadeAction::Wait => Fade::Idle,
        };
        let incoming_gain = preload.gain;
        self.phase = Phase::Playing(Playing {
            next: Next::Crossfading { preload, fade },
            ..playing
        });
        let io = match fade {
            Fade::Fading(fraction) => EngineEffect::Fade {
                outgoing: self.volume() * gain_out(fraction),
                incoming: effective_volume(
                    &self.config,
                    incoming_gain,
                    self.user_factor.ratio(),
                ) * gain_in(fraction),
                at: position,
            },
            Fade::Idle => self.position_event(position),
        };
        (Engine::Live(self), io)
    }
}

fn gapless_event(
    previous_queue_len: usize,
    queue_len: usize,
    position: Duration,
) -> Option<AudioEvent> {
    match (queue_len, previous_queue_len) {
        (0, 0) => None,
        (0, _) => Some(AudioEvent::Ended),
        (len, prev) if prev > len => Some(AudioEvent::TrackChanged),
        (_, _) => Some(AudioEvent::Position(position)),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{AudioEvent, domain::Speed, update::Machine};
    use rstest::rstest;

    use crate::engine::{
        crossfade::{gain_in, gain_out},
        effect::{EngineEffect, EngineMessage},
        phase::{Fade, Handover, Incoming, Next, Outgoing, Phase, Playing},
        state::{
            Engine,
            Live,
            fixtures::{
                CROSSFADE_SECONDS,
                TOTAL,
                crossfade,
                crossfading,
                handing_over,
                landed,
                live,
                live_with_crossfade,
                load,
                load_at,
                loading,
                loading_track,
                observed,
                playing,
                playing_track,
                preload,
                preload_at,
                preload_b,
                promoted,
                retiring,
                second,
                secs,
                set_crossfade,
                third,
                track_a,
                track_b,
            },
        },
    };

    struct Transition {
        next: Engine,
        io: EngineEffect,
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

    #[rstest]
    #[case::idle_tick_is_silent(
        Engine::Live(live()),
        observed(0, Duration::ZERO),
        Transition { next: Engine::Live(live()), io: EngineEffect::Nothing }
    )]
    #[case::playing_tick_reports_position(
        Engine::Live(playing()),
        observed(1, secs(5)),
        Transition {
            next: Engine::Live(Live { last_position: Some(secs(5)), ..playing() }),
            io: EngineEffect::Send(AudioEvent::Position(secs(5))),
        }
    )]
    #[case::a_shorter_queue_is_a_gapless_hand_off(
        Engine::Live(Live { phase: Phase::Playing(Playing { previous_queue_len: 2, ..Playing::new(track_a()) }), ..playing() }),
        observed(1, Duration::ZERO),
        Transition { next: Engine::Live(playing()), io: EngineEffect::Send(AudioEvent::TrackChanged) }
    )]
    #[case::a_drained_queue_is_the_end(
        Engine::Live(playing()),
        observed(0, TOTAL),
        Transition {
            next: Engine::Live(Live { phase: Phase::Playing(Playing::new(track_a())), ..playing() }),
            io: EngineEffect::Send(AudioEvent::Ended),
        }
    )]
    #[case::before_the_fade_window_the_preload_stays_silent(
        Engine::Live(crossfading(Fade::Idle)),
        observed(1, secs(50)),
        Transition {
            next: Engine::Live(Live { last_position: Some(secs(50)), ..crossfading(Fade::Idle) }),
            io: EngineEffect::Send(AudioEvent::Position(secs(50))),
        }
    )]
    #[case::inside_the_window_both_sinks_fade(
        Engine::Live(crossfading(Fade::Idle)),
        observed(1, secs(92)),
        Transition {
            next: Engine::Live(crossfading(Fade::Fading(0.2))),
            io: EngineEffect::Fade { outgoing: gain_out(0.2), incoming: gain_in(0.2), at: secs(92) },
        }
    )]
    #[case::a_seek_back_out_of_the_window_restores_the_volumes(
        Engine::Live(crossfading(Fade::Fading(0.2))),
        observed(1, secs(80)),
        Transition {
            next: Engine::Live(Live { last_position: Some(secs(80)), ..crossfading(Fade::Idle) }),
            io: EngineEffect::Send(AudioEvent::Position(secs(80))),
        }
    )]
    #[case::a_completed_fade_hands_off(
        Engine::Live(crossfading(Fade::Fading(0.9))),
        observed(1, TOTAL),
        Transition { next: Engine::Live(promoted(crossfade(10))), io: EngineEffect::Promote { volume: 1.0 } }
    )]
    #[case::a_drained_primary_hands_off_at_once(
        Engine::Live(crossfading(Fade::Idle)),
        observed(0, secs(30)),
        Transition { next: Engine::Live(promoted(crossfade(10))), io: EngineEffect::Promote { volume: 1.0 } }
    )]
    #[case::a_retiring_streams_first_tick_starts_the_ramp(
        Engine::Live(retiring(0.0)),
        observed(1, Duration::ZERO),
        Transition {
            next: Engine::Live(retiring(0.0)),
            io: EngineEffect::Retire { playing: gain_in(0.0), retiring: gain_out(0.0), at: Duration::ZERO },
        }
    )]
    #[case::the_middle_of_a_retiring_ramp_is_equal_power(
        Engine::Live(retiring(0.0)),
        observed(1, secs(5)),
        Transition {
            next: Engine::Live(retiring(0.5)),
            io: EngineEffect::Retire { playing: gain_in(0.5), retiring: gain_out(0.5), at: secs(5) },
        }
    )]
    #[case::a_retiring_stream_is_dropped_when_the_ramp_ends(
        Engine::Live(retiring(0.9)),
        observed(1, secs(10)),
        Transition {
            next: Engine::Live(Live { phase: playing_track(track_b()), ..retiring(0.9) }),
            io: EngineEffect::Retired { playing: gain_in(1.0), at: secs(10) },
        }
    )]
    #[case::a_still_decoding_skip_holds_the_retiring_stream_where_it_was(
        Engine::Live(handing_over(
            Outgoing { from: 1.0, fraction: 0.0 },
            Incoming::Loading(loading_track("/b")),
        )),
        observed(0, Duration::ZERO),
        Transition {
            next: Engine::Live(handing_over(
                Outgoing { from: 1.0, fraction: 0.0 },
                Incoming::Loading(loading_track("/b")),
            )),
            io: EngineEffect::Retire { playing: 0.0, retiring: 1.0, at: Duration::ZERO },
        }
    )]
    #[case::a_tick_while_decoding_is_silent(
        Engine::Live(loading()),
        observed(0, Duration::ZERO),
        Transition { next: Engine::Live(loading()), io: EngineEffect::Nothing }
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

    #[test]
    fn set_crossfade_is_read_by_the_next_tick() {
        let (mut state, _) = trace(
            Engine::Live(playing()),
            vec![set_crossfade(10), preload("/b"), landed(preload_b())],
        );
        assert_eq!(
            state.update(observed(1, secs(92))),
            Ok(EngineEffect::Fade {
                outgoing: gain_out(0.2),
                incoming: gain_in(0.2),
                at: secs(92)
            })
        );
    }

    #[test]
    fn the_preload_sink_is_paused_until_the_fade_opens() {
        let mut state = Engine::Live(playing());
        state.update(set_crossfade(10)).unwrap();
        assert_eq!(
            state.update(preload("/b")),
            Ok(EngineEffect::PreloadCrossfade {
                path: "/b".into(),
                gain: None,
                speed: Speed::default()
            })
        );

        assert_eq!(state.update(landed(preload_b())), Ok(EngineEffect::Nothing));

        assert_eq!(
            state.update(observed(1, secs(85))),
            Ok(EngineEffect::Send(AudioEvent::Position(secs(85))))
        );

        assert_eq!(
            state.update(observed(1, secs(92))),
            Ok(EngineEffect::Fade {
                outgoing: gain_out(0.2),
                incoming: gain_in(0.2),
                at: secs(92)
            })
        );
    }

    fn u64_to_f32(value: u64) -> f32 {
        f32::from(u16::try_from(value).unwrap_or(u16::MAX))
    }

    fn still_live(state: Engine) -> Live {
        match state {
            Engine::Live(live) => live,
            Engine::Muted(_) => panic!("the engine stays live"),
        }
    }

    fn crossfading_on_config() -> (Engine, Vec<EngineEffect>) {
        trace(
            Engine::Live(live_with_crossfade(CROSSFADE_SECONDS)),
            vec![
                load("/a"),
                EngineMessage::Decoded(Ok(Some(TOTAL))),
                preload_at("/b", second()),
                landed(preload_b()),
            ],
        )
    }

    fn playing_a() -> (Engine, Vec<EngineEffect>) {
        trace(
            Engine::Live(live_with_crossfade(CROSSFADE_SECONDS)),
            vec![load("/a"), EngineMessage::Decoded(Ok(Some(TOTAL)))],
        )
    }

    #[test]
    fn the_configured_crossfade_opens_a_second_sink() {
        let (state, log) = crossfading_on_config();
        insta::assert_debug_snapshot!(log);
        let live = still_live(state);
        assert!(matches!(
            live.phase,
            Phase::Playing(Playing {
                next: Next::Crossfading { .. },
                ..
            })
        ));
    }

    #[test]
    fn the_last_seconds_ramp_both_sinks() {
        let (state, mut log) = crossfading_on_config();
        let fade_start = TOTAL - secs(CROSSFADE_SECONDS);
        let midpoint = fade_start + secs(CROSSFADE_SECONDS) / 2;
        let (state, tail) = trace(
            state,
            vec![
                observed(1, fade_start - secs(1)),
                observed(1, midpoint),
                observed(1, TOTAL),
            ],
        );
        log.extend(tail);
        insta::assert_debug_snapshot!(log);
        let _ = still_live(state);
    }

    #[test]
    fn a_skip_fades_into_the_next_track_instead_of_cutting() {
        let (state, mut log) = playing_a();
        let (state, loaded_tail) = trace(state, vec![load_at("/b", second())]);
        log.extend(loaded_tail);

        let (state, decoded_tail) =
            trace(state, vec![EngineMessage::Decoded(Ok(Some(secs(180))))]);
        log.extend(decoded_tail);

        let mut state = state;
        for tick in 0..CROSSFADE_SECONDS {
            let (next, observed_tail) = trace(state, vec![observed(1, secs(tick))]);
            state = next;
            log.extend(observed_tail);
        }

        let (state, final_tail) =
            trace(state, vec![observed(1, secs(CROSSFADE_SECONDS))]);
        log.extend(final_tail);
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert!(matches!(live.phase, Phase::Playing(_)), "{:?}", live.phase);
    }

    #[test]
    fn a_second_skip_inside_a_fade_keeps_two_streams() {
        let (state, mut log) = playing_a();
        let (state, loaded_tail) = trace(state, vec![load_at("/b", second())]);
        log.extend(loaded_tail);
        let (state, decoded_tail) =
            trace(state, vec![EngineMessage::Decoded(Ok(Some(secs(180))))]);
        log.extend(decoded_tail);
        let (state, observed_tail) = trace(state, vec![observed(1, secs(3))]);
        log.extend(observed_tail);

        let (state, second_load_tail) = trace(state, vec![load_at("/c", third())]);
        log.extend(second_load_tail);
        insta::assert_debug_snapshot!(log);

        let Phase::Handover(Handover { outgoing, .. }) = still_live(state).phase else {
            panic!("a second skip inside a fade hands over again");
        };
        let expected = gain_in(3.0 / u64_to_f32(CROSSFADE_SECONDS));
        assert!(
            (outgoing.from - expected).abs() < 1e-3 && outgoing.fraction == 0.0,
            "the new fade starts from the gain the last one had reached, got {outgoing:?}"
        );
    }

    #[test]
    fn a_zero_crossfade_cuts_instead_of_fading() {
        let (state, log) = trace(
            Engine::Live(live_with_crossfade(0)),
            vec![
                load("/a"),
                EngineMessage::Decoded(Ok(Some(TOTAL))),
                load_at("/b", second()),
            ],
        );
        insta::assert_debug_snapshot!(log);

        let live = still_live(state);
        assert!(matches!(live.phase, Phase::Loading(_)), "{:?}", live.phase);
    }
}
