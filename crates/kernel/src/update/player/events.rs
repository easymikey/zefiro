use std::{mem, sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, LibraryCmd, PlaybackChange, TrackLoad},
    domain::{Moment, PausedBy, Player, Playhead, Preload, Revision, Track},
    message::AudioError,
    update::player::{
        Anchor,
        PlayerError,
        Stamp,
        StartOrigin,
        handover_effects,
        seek_effect,
    },
};

#[derive(Debug)]
pub struct Lookahead {
    pub preload_lead: Duration,
    pub ab_loop: Option<(Duration, Duration)>,
    pub next: Option<Arc<Track>>,
    pub duration: Duration,
    pub now: Moment,
    pub revision: Revision,
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

    fn preloading(self, at: Duration, preload: &mut Preload) -> Cmd {
        match self.next {
            Some(next) if self.is_preload_due(at) => {
                let cmd = Cmd::Batch(vec![
                    Effect::Audio(AudioCmd::Preload(TrackLoad::for_track(
                        &next,
                        self.revision,
                    ))),
                    Effect::Library(LibraryCmd::PrefetchCover(
                        next.path().to_path_buf(),
                    )),
                ]);
                *preload = Preload::Queued(next);
                cmd
            }
            Some(_) | None => Cmd::None,
        }
    }
}

impl Player {
    pub(crate) fn loaded(
        &mut self,
        total: Option<Duration>,
        anchor: Anchor,
    ) -> Result<Cmd, PlayerError> {
        match mem::replace(self, Player::Stopped) {
            Player::Loading { track, at } => {
                let track = match total {
                    Some(total) => Arc::new(track.with_duration(total)),
                    None => track,
                };
                *self = Player::Playing {
                    track,
                    head: Playhead::anchored(at, anchor.since, anchor.speed),
                    preload: Preload::None,
                };
                Ok(Cmd::None)
            }
            other @ (Player::Playing { .. }
            | Player::Paused { .. }
            | Player::Stopped) => {
                let refusal = other.refusal();
                *self = other;
                Err(refusal)
            }
        }
    }

    pub(crate) fn failed(&mut self, failure: &AudioError, now: Moment) -> Cmd {
        match failure {
            AudioError::OutputLost { .. } => self.output_lost(now),
            AudioError::Decode { .. }
            | AudioError::Device { .. }
            | AudioError::Stream { .. }
            | AudioError::Preload { .. } => self.load_failed(),
            AudioError::Seek { .. } => Cmd::None,
        }
    }

    fn output_lost(&mut self, now: Moment) -> Cmd {
        match self {
            Player::Playing { .. } => self.pause(now, PausedBy::Listener),
            Player::Loading { .. } => self.stop(),
            Player::Paused { .. } | Player::Stopped => Cmd::None,
        }
    }

    fn load_failed(&mut self) -> Cmd {
        match self {
            Player::Loading { .. } => self.stop(),
            Player::Playing { .. } | Player::Paused { .. } | Player::Stopped => {
                Cmd::None
            }
        }
    }

    pub(crate) fn positioned(
        &mut self,
        offset: Duration,
        lookahead: Lookahead,
    ) -> Result<Cmd, PlayerError> {
        match (&mut *self, lookahead.loop_start(offset)) {
            (Player::Playing { head, preload, .. }, Some(a)) => {
                *head = Playhead::anchored(a, lookahead.now, head.speed);
                *preload = mem::replace(preload, Preload::None).seek_reset();
                Ok(seek_effect(a))
            }
            (Player::Playing { head, preload, .. }, None) => {
                *head = Playhead::anchored(offset, lookahead.now, head.speed);
                match preload {
                    Preload::None => Ok(lookahead.preloading(offset, preload)),
                    Preload::Queued(_) | Preload::Stale(_) => Ok(Cmd::None),
                }
            }
            (Player::Paused { at, .. }, Some(a)) => {
                *at = a;
                Ok(seek_effect(a))
            }
            (Player::Paused { .. }, None) => Err(PlayerError::Paused),
            (Player::Loading { .. }, Some(_) | None) => Err(PlayerError::Loading),
            (Player::Stopped, Some(_) | None) => Err(PlayerError::Stopped),
        }
    }

    pub(crate) fn reported(
        &mut self,
        offset: Duration,
        now: Moment,
    ) -> Result<Cmd, PlayerError> {
        match self {
            Player::Playing { head, .. } => {
                *head = Playhead::anchored(offset, now, head.speed);
                Ok(Cmd::None)
            }
            Player::Paused { .. } | Player::Loading { .. } | Player::Stopped => {
                Err(self.refusal())
            }
        }
    }

    pub(crate) fn track_changed(
        &mut self,
        next: Option<Arc<Track>>,
        now: Moment,
    ) -> Result<Cmd, PlayerError> {
        match self {
            Player::Playing {
                track,
                head,
                preload,
            } => {
                if let Some(next) = next {
                    *track = next;
                }
                *head = Playhead::anchored(Duration::ZERO, now, head.speed);
                *preload = Preload::None;
                Ok(handover_effects(track, PlaybackChange::Play, now))
            }
            Player::Paused { track, at, .. } => {
                if let Some(next) = next {
                    *track = next;
                }
                *at = Duration::ZERO;
                Ok(handover_effects(track, PlaybackChange::Pause, now))
            }
            Player::Loading { .. } | Player::Stopped => Err(self.refusal()),
        }
    }

    pub(crate) fn ended(
        &mut self,
        next: Option<Arc<Track>>,
        stamp: Stamp,
    ) -> Result<Cmd, PlayerError> {
        match self {
            Player::Playing { .. } => Ok(match next {
                Some(track) => self.start(track, StartOrigin::TrackEnded(stamp)),
                None => self.stop(),
            }),
            Player::Paused { .. } | Player::Loading { .. } | Player::Stopped => {
                Err(self.refusal())
            }
        }
    }
}

#[must_use]
pub(crate) fn next_decision(head: Playhead, lookahead: &Lookahead) -> Option<Duration> {
    let current = head.position_at(lookahead.now);
    let target = [
        lookahead.preload_due_at(),
        lookahead.ab_loop.map(|(_, b)| b),
    ]
    .into_iter()
    .flatten()
    .filter(|&point| point > current)
    .min()?;
    Some((target - current).div_f32(head.speed.get()))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        domain::{
            AudioFormat,
            Bounded,
            Moment,
            Playhead,
            Revision,
            Speed,
            Tags,
            Track,
        },
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
            revision: Revision::default(),
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
        let delay = next_decision(head, &lookahead);
        assert_eq!(delay, expected_secs.map(Duration::from_secs));
    }
}
