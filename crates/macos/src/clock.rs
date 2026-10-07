#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use kernel::{cmd::Playback, domain::speed::Speed};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NowPlayingClock {
    Paused {
        offset: Duration,
        speed: Speed,
    },
    Playing {
        offset: Duration,
        started_at: Instant,
        speed: Speed,
    },
}

impl Default for NowPlayingClock {
    fn default() -> Self {
        Self::Paused {
            offset: Duration::ZERO,
            speed: Speed::default(),
        }
    }
}

impl NowPlayingClock {
    pub(crate) fn playback(self) -> Playback {
        match self {
            Self::Paused { .. } => Playback::Paused,
            Self::Playing { .. } => Playback::Playing,
        }
    }

    pub(crate) fn speed(self) -> Speed {
        match self {
            Self::Paused {
                speed,
                offset: _offset,
            } => speed,
            Self::Playing {
                speed,
                offset: _offset,
                started_at: _started_at,
            } => speed,
        }
    }

    pub(crate) fn elapsed(self, now: Instant) -> Duration {
        match self {
            Self::Paused {
                offset,
                speed: _speed,
            } => offset,
            Self::Playing {
                offset,
                started_at,
                speed,
            } => offset.saturating_add(
                now.saturating_duration_since(started_at)
                    .mul_f32(speed.get()),
            ),
        }
    }

    pub(crate) fn seek(self, to: Duration, now: Instant) -> Self {
        Self::Paused {
            offset: to,
            speed: self.speed(),
        }
        .change_playback(self.playback(), now)
    }

    pub(crate) fn change_playback(self, playback: Playback, now: Instant) -> Self {
        let (offset, speed) = (self.elapsed(now), self.speed());
        match playback {
            Playback::Paused => Self::Paused { offset, speed },
            Playback::Playing => Self::Playing {
                offset,
                started_at: now,
                speed,
            },
        }
    }

    pub(crate) fn at_speed(self, speed: Speed, now: Instant) -> Self {
        Self::Paused {
            offset: self.elapsed(now),
            speed,
        }
        .change_playback(self.playback(), now)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::{
        cmd::Playback,
        domain::{bounded::Bounded, speed::Speed},
    };

    use crate::clock::NowPlayingClock;

    #[test]
    fn the_now_playing_clock_only_runs_while_playing() {
        let start = Instant::now();
        let later = start + Duration::from_secs(30);
        let paused = NowPlayingClock::default().seek(Duration::from_secs(5), start);
        assert_eq!(paused.elapsed(later), Duration::from_secs(5));

        let playing = paused.change_playback(Playback::Playing, start);
        assert_eq!(playing.elapsed(later), Duration::from_secs(35));
    }

    #[test]
    fn a_seek_re_anchors_the_now_playing_clock() {
        let start = Instant::now();
        let mid = start + Duration::from_secs(10);
        let clock = NowPlayingClock::default()
            .change_playback(Playback::Playing, start)
            .seek(Duration::from_secs(90), mid);
        assert_eq!(clock.elapsed(mid), Duration::from_secs(90));
        assert_eq!(
            clock.elapsed(mid + Duration::from_secs(3)),
            Duration::from_secs(93)
        );
    }

    #[test]
    fn pausing_freezes_the_now_playing_clock_where_playback_reached() {
        let start = Instant::now();
        let paused_at = start + Duration::from_secs(12);
        let clock = NowPlayingClock::default()
            .change_playback(Playback::Playing, start)
            .change_playback(Playback::Paused, paused_at);
        assert_eq!(
            clock.elapsed(paused_at + Duration::from_secs(60)),
            Duration::from_secs(12)
        );
    }

    #[test]
    fn a_clock_playing_at_twice_the_speed_advances_two_seconds_a_second() {
        let start = Instant::now();
        let clock = NowPlayingClock::default()
            .at_speed(Speed::clamped(2.0), start)
            .change_playback(Playback::Playing, start);
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(1)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn a_speed_change_re_anchors_where_playback_reached() {
        let start = Instant::now();
        let changed_at = start + Duration::from_secs(10);
        let clock = NowPlayingClock::default()
            .change_playback(Playback::Playing, start)
            .at_speed(Speed::clamped(2.0), changed_at);
        assert_eq!(clock.elapsed(changed_at), Duration::from_secs(10));
        assert_eq!(
            clock.elapsed(changed_at + Duration::from_secs(3)),
            Duration::from_secs(16)
        );
    }

    #[test]
    fn a_paused_clock_keeps_its_speed_for_the_resume() {
        let start = Instant::now();
        let clock = NowPlayingClock::default()
            .at_speed(Speed::clamped(0.5), start)
            .change_playback(Playback::Playing, start);
        assert_eq!(clock.speed(), Speed::clamped(0.5));
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(4)),
            Duration::from_secs(2)
        );
    }
}
