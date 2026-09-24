#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use kernel::Playback;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanelClock {
    anchor: Duration,
    since: Instant,
    playback: Playback,
}

impl PanelClock {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            anchor: Duration::ZERO,
            since: now,
            playback: Playback::Paused,
        }
    }

    pub(crate) fn playback(self) -> Playback {
        self.playback
    }

    pub(crate) fn elapsed(self, now: Instant) -> Duration {
        match self.playback {
            Playback::Playing => self
                .anchor
                .saturating_add(now.saturating_duration_since(self.since)),
            Playback::Paused => self.anchor,
        }
    }

    pub(crate) fn seek(self, to: Duration, now: Instant) -> Self {
        Self {
            anchor: to,
            since: now,
            playback: self.playback,
        }
    }

    pub(crate) fn with_playback(self, playback: Playback, now: Instant) -> Self {
        Self {
            anchor: self.elapsed(now),
            since: now,
            playback,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::Playback;

    use crate::clock::PanelClock;

    #[test]
    fn the_panel_clock_only_runs_while_playing() {
        let start = Instant::now();
        let later = start + Duration::from_secs(30);
        let paused = PanelClock::new(start).seek(Duration::from_secs(5), start);
        assert_eq!(paused.elapsed(later), Duration::from_secs(5));

        let playing = paused.with_playback(Playback::Playing, start);
        assert_eq!(playing.elapsed(later), Duration::from_secs(35));
    }

    #[test]
    fn a_seek_re_anchors_the_panel_clock() {
        let start = Instant::now();
        let mid = start + Duration::from_secs(10);
        let clock = PanelClock::new(start)
            .with_playback(Playback::Playing, start)
            .seek(Duration::from_secs(90), mid);
        assert_eq!(clock.elapsed(mid), Duration::from_secs(90));
        assert_eq!(
            clock.elapsed(mid + Duration::from_secs(3)),
            Duration::from_secs(93)
        );
    }

    #[test]
    fn pausing_freezes_the_panel_clock_where_playback_reached() {
        let start = Instant::now();
        let paused_at = start + Duration::from_secs(12);
        let clock = PanelClock::new(start)
            .with_playback(Playback::Playing, start)
            .with_playback(Playback::Paused, paused_at);
        assert_eq!(
            clock.elapsed(paused_at + Duration::from_secs(60)),
            Duration::from_secs(12)
        );
    }
}
