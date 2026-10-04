#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use kernel::Playback;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NowPlayingClock {
    Paused(Duration),
    Playing { anchor: Duration, since: Instant },
}

impl Default for NowPlayingClock {
    fn default() -> Self {
        Self::Paused(Duration::ZERO)
    }
}

impl NowPlayingClock {
    pub(crate) fn playback(self) -> Playback {
        match self {
            Self::Paused(_) => Playback::Paused,
            Self::Playing { .. } => Playback::Playing,
        }
    }

    pub(crate) fn elapsed(self, now: Instant) -> Duration {
        match self {
            Self::Paused(position) => position,
            Self::Playing { anchor, since } => {
                anchor.saturating_add(now.saturating_duration_since(since))
            }
        }
    }

    pub(crate) fn seek(self, to: Duration, now: Instant) -> Self {
        Self::at(self.playback(), to, now)
    }

    pub(crate) fn change_playback(self, playback: Playback, now: Instant) -> Self {
        Self::at(playback, self.elapsed(now), now)
    }

    fn at(playback: Playback, position: Duration, now: Instant) -> Self {
        match playback {
            Playback::Paused => Self::Paused(position),
            Playback::Playing => Self::Playing {
                anchor: position,
                since: now,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::Playback;

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
}
