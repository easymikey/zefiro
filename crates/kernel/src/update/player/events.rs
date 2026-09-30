use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, LibraryCmd, PlaybackChange},
    domain::{Moment, Pause, Player, Playhead, Preload, Revision, Track},
    message::AudioError,
    update::player::{
        Anchor,
        StartOrigin,
        Transition,
        handoff_effects,
        seek_effect,
        start,
        stopped_effects,
    },
};

#[derive(Debug)]
pub struct Lookahead {
    pub preload_lead: Duration,
    pub ab_loop: Option<(Duration, Duration)>,
    pub next: Option<Arc<Track>>,
    pub duration: Duration,
    pub now: Moment,
}

impl Lookahead {
    fn loop_start(&self, at: Duration) -> Option<Duration> {
        self.ab_loop.and_then(|(a, b)| (at >= b).then_some(a))
    }

    fn preload_due_at(&self) -> Option<Duration> {
        (self.next.is_some() && !self.duration.is_zero())
            .then(|| self.duration.saturating_sub(self.preload_lead))
    }

    fn is_preload_due(&self, at: Duration) -> bool {
        self.preload_due_at().is_some_and(|due| at >= due)
    }

    fn preloading(self, track: Arc<Track>, head: Playhead) -> (Player, Cmd) {
        let at = head.offset;
        let (preload, cmd) = match self.next {
            Some(next) if self.is_preload_due(at) => {
                let cmd = Cmd::Batch(vec![
                    Effect::Audio(AudioCmd::Preload {
                        path: next.path().to_path_buf(),
                        gain: next.audio_format().replay_gain,
                        revision: Revision::UNSTAMPED,
                    }),
                    Effect::Library(LibraryCmd::PrefetchCover(
                        next.path().to_path_buf(),
                    )),
                ]);
                (Preload::Queued(next), cmd)
            }
            Some(_) | None => (Preload::None, Cmd::None),
        };
        (
            Player::Playing {
                track,
                head,
                preload,
            },
            cmd,
        )
    }
}

impl Player {
    pub(crate) fn loaded(self, total: Option<Duration>, anchor: Anchor) -> Transition {
        match self {
            Player::Loading { track, at } => {
                let track = match total {
                    Some(total) => Arc::new(track.with_duration(total)),
                    None => track,
                };
                let head = Playhead::anchored(at, anchor.now, anchor.speed);
                Ok((
                    Player::Playing {
                        track,
                        head,
                        preload: Preload::None,
                    },
                    Cmd::None,
                ))
            }
            other @ (Player::Playing { .. }
            | Player::Paused { .. }
            | Player::Stopped) => other.refuse(),
        }
    }

    pub(crate) fn failed(self, failure: &AudioError, now: Moment) -> (Player, Cmd) {
        match failure {
            AudioError::OutputLost { .. } => self.output_lost(now),
            AudioError::Decode { .. }
            | AudioError::Device { .. }
            | AudioError::Stream { .. }
            | AudioError::Preload { .. } => self.load_failed(),
            AudioError::Seek { .. } => (self, Cmd::None),
        }
    }

    fn output_lost(self, now: Moment) -> (Player, Cmd) {
        match self {
            Player::Playing { track, head, .. } => (
                Player::Paused {
                    track,
                    at: head.position_at(now),
                    pause: Pause::ByListener,
                },
                PlaybackChange::Pause.cued(),
            ),
            Player::Loading { .. } => (Player::Stopped, stopped_effects()),
            other @ (Player::Paused { .. } | Player::Stopped) => (other, Cmd::None),
        }
    }

    fn load_failed(self) -> (Player, Cmd) {
        match self {
            Player::Loading { .. } => (Player::Stopped, stopped_effects()),
            other @ (Player::Playing { .. }
            | Player::Paused { .. }
            | Player::Stopped) => (other, Cmd::None),
        }
    }

    pub(crate) fn positioned(
        self,
        offset: Duration,
        lookahead: Lookahead,
    ) -> Transition {
        match (self, lookahead.loop_start(offset)) {
            (
                Player::Playing {
                    track,
                    head,
                    preload,
                },
                Some(a),
            ) => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(a, lookahead.now, head.speed),
                    preload: preload.seek_reset(),
                },
                seek_effect(a),
            )),
            (
                Player::Playing {
                    track,
                    head,
                    preload: Preload::None,
                },
                None,
            ) => {
                let anchored = Playhead::anchored(offset, lookahead.now, head.speed);
                Ok(lookahead.preloading(track, anchored))
            }
            (
                Player::Playing {
                    track,
                    head,
                    preload,
                },
                None,
            ) => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(offset, lookahead.now, head.speed),
                    preload,
                },
                Cmd::None,
            )),
            (Player::Paused { track, pause, .. }, Some(a)) => Ok((
                Player::Paused {
                    track,
                    at: a,
                    pause,
                },
                seek_effect(a),
            )),
            (paused @ Player::Paused { .. }, None) => paused.refuse(),
            (other @ (Player::Loading { .. } | Player::Stopped), Some(_) | None) => {
                other.refuse()
            }
        }
    }

    pub(crate) fn reported(self, offset: Duration, now: Moment) -> Transition {
        match self {
            Player::Playing {
                track,
                head,
                preload,
            } => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(offset, now, head.speed),
                    preload,
                },
                Cmd::None,
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => other.refuse(),
        }
    }

    pub(crate) fn track_changed(
        self,
        next: Option<Arc<Track>>,
        now: Moment,
    ) -> Transition {
        match self {
            Player::Playing { track, head, .. } => {
                let track = next.unwrap_or(track);
                let effects = handoff_effects(&track, PlaybackChange::Play);
                Ok((
                    Player::Playing {
                        track,
                        head: Playhead::anchored(Duration::ZERO, now, head.speed),
                        preload: Preload::None,
                    },
                    effects,
                ))
            }
            Player::Paused { track, pause, .. } => {
                let track = next.unwrap_or(track);
                let effects = handoff_effects(&track, PlaybackChange::Pause);
                Ok((
                    Player::Paused {
                        track,
                        at: Duration::ZERO,
                        pause,
                    },
                    effects,
                ))
            }
            other @ (Player::Loading { .. } | Player::Stopped) => other.refuse(),
        }
    }

    pub(crate) fn ended(self, next: Option<Arc<Track>>) -> Transition {
        match self {
            Player::Playing { .. } => Ok(next.map_or_else(
                || (Player::Stopped, stopped_effects()),
                |track| start(track, StartOrigin::TrackEnded),
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => other.refuse(),
        }
    }
}

#[must_use]
pub(crate) fn next_decision(
    head: Playhead,
    lookahead: &Lookahead,
    now: Moment,
) -> Option<Duration> {
    let current = head.position_at(now);
    let target = [
        lookahead.preload_due_at(),
        lookahead.ab_loop.map(|(_, b)| b),
    ]
    .into_iter()
    .flatten()
    .filter(|&point| point > current)
    .min()?;
    Some((target - current).div_f32(head.speed.value()))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        domain::{AudioFormat, Bounded, Moment, Playhead, Speed, Tags, Track},
        update::player::events::{Lookahead, next_decision},
    };

    fn head_at(offset: u64, speed: f32) -> Playhead {
        Playhead::anchored(
            Duration::from_secs(offset),
            Moment::new(Duration::ZERO),
            Speed::clamped(speed),
        )
    }

    fn a_track() -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path("/tmp/next.flac")
                .duration(Duration::from_secs(1))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

    struct Setup {
        preload_lead: u64,
        ab_loop: Option<(u64, u64)>,
        next: Option<Arc<Track>>,
        duration: u64,
    }

    fn lookahead(setup: Setup) -> Lookahead {
        Lookahead {
            preload_lead: Duration::from_secs(setup.preload_lead),
            ab_loop: setup
                .ab_loop
                .map(|(a, b)| (Duration::from_secs(a), Duration::from_secs(b))),
            next: setup.next,
            duration: Duration::from_secs(setup.duration),
            now: Moment::new(Duration::ZERO),
        }
    }

    #[rstest]
    #[case::no_decision_ahead_arms_nothing(
        head_at(0, 1.0),
        lookahead(Setup { preload_lead: 10, ab_loop: None, next: None, duration: 0 }),
        None
    )]
    #[case::preload_due_point_arms_at_unity_speed(
        head_at(0, 1.0),
        lookahead(Setup { preload_lead: 10, ab_loop: None, next: Some(a_track()), duration: 100 }),
        Some(90)
    )]
    #[case::ab_b_point_arms_when_earlier_than_preload(
        head_at(0, 1.0),
        lookahead(Setup { preload_lead: 10, ab_loop: Some((5, 20)), next: Some(a_track()), duration: 100 }),
        Some(20)
    )]
    #[case::double_speed_halves_the_wait(
        head_at(0, 2.0),
        lookahead(Setup { preload_lead: 10, ab_loop: None, next: Some(a_track()), duration: 100 }),
        Some(45)
    )]
    #[case::half_speed_doubles_the_wait(
        head_at(0, 0.5),
        lookahead(Setup { preload_lead: 10, ab_loop: None, next: Some(a_track()), duration: 100 }),
        Some(180)
    )]
    #[case::a_past_decision_is_not_armed(
        head_at(95, 1.0),
        lookahead(Setup { preload_lead: 10, ab_loop: None, next: Some(a_track()), duration: 100 }),
        None
    )]
    #[case::no_next_track_skips_the_preload_point(
        head_at(0, 1.0),
        lookahead(Setup { preload_lead: 10, ab_loop: None, next: None, duration: 100 }),
        None
    )]
    fn next_decision_arms_the_earlier_of_preload_or_ab(
        #[case] head: Playhead,
        #[case] lookahead: Lookahead,
        #[case] expected_secs: Option<u64>,
    ) {
        let delay = next_decision(head, &lookahead, Moment::new(Duration::ZERO));
        assert_eq!(delay, expected_secs.map(Duration::from_secs));
    }
}
