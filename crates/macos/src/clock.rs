#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use kernel::{cmd::Playback, domain::speed::Speed};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct NowPlayingClock {
    offset: Duration,
    speed: Speed,
    run: Run,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Run {
    #[default]
    Paused,
    Playing(Instant),
}

impl Run {
    fn restarted(self, now: Instant) -> Self {
        match self {
            Self::Paused => Self::Paused,
            Self::Playing(_) => Self::Playing(now),
        }
    }
}

impl NowPlayingClock {
    pub(crate) fn playback(self) -> Playback {
        match self.run {
            Run::Paused => Playback::Paused,
            Run::Playing(_) => Playback::Playing,
        }
    }

    pub(crate) fn speed(self) -> Speed {
        self.speed
    }

    pub(crate) fn elapsed(self, now: Instant) -> Duration {
        match self.run {
            Run::Paused => self.offset,
            Run::Playing(started_at) => self.offset.saturating_add(
                now.saturating_duration_since(started_at)
                    .mul_f32(self.speed.get()),
            ),
        }
    }

    pub(crate) fn seek(self, to: Duration, now: Instant) -> Self {
        Self {
            offset: to,
            speed: self.speed,
            run: self.run.restarted(now),
        }
    }

    pub(crate) fn change_playback(self, playback: Playback, now: Instant) -> Self {
        let run = match playback {
            Playback::Paused => Run::Paused,
            Playback::Playing => Run::Playing(now),
        };
        Self {
            offset: self.elapsed(now),
            speed: self.speed,
            run,
        }
    }

    pub(crate) fn at_speed(self, speed: Speed, now: Instant) -> Self {
        Self {
            offset: self.elapsed(now),
            speed,
            run: self.run.restarted(now),
        }
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
