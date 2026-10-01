use std::path::PathBuf;

use kernel::{Moment, Player};
use ratatui::{layout::Rect, style::Color};
use runtime::CoverRequest;
use terminal::CrossfadePermit;
use widgets::{OnScreen, Presence, Spectrum, SpectrumMotion, SpectrumSmoothing};

use crate::shell::cover_crossfade::PendingCrossfade;

pub(crate) struct Motion {
    pub(in crate::shell) first_paint: Moment,
    pub(in crate::shell) last_paint: Moment,
    pub(in crate::shell) area: Rect,
    pub(in crate::shell) spectrum_smoothing: SpectrumSmoothing,
    pub(in crate::shell) spectrum_motion: SpectrumMotion,
    pub(in crate::shell) spectrum_advanced_at: Moment,
    pub(in crate::shell) playlist_body_height: u16,
    pub(in crate::shell) wanted_cover: Option<PathBuf>,
    pub(in crate::shell) pending_crossfade: PendingCrossfade,
    pub(in crate::shell) on_screen: OnScreen,
    pub(in crate::shell) screen_clear: ScreenClear,
    pub(in crate::shell) outgoing_theme_background: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ScreenClear {
    NotDue,
    Due,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpectrumFeed {
    Live,
    Silent,
}

impl SpectrumFeed {
    pub(crate) fn of(player: &Player) -> Self {
        if player.is_playing() {
            Self::Live
        } else {
            Self::Silent
        }
    }
}

pub(crate) struct Advance {
    pub(crate) cover: Option<CoverRequest>,
    pub(crate) visible_rows: Option<usize>,
    pub(crate) crossfade: CrossfadePermit,
    pub(crate) screen_clear: ScreenClear,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            first_paint: Moment::default(),
            last_paint: Moment::default(),
            area: Rect::default(),
            spectrum_smoothing: SpectrumSmoothing::default(),
            spectrum_motion: SpectrumMotion::Settled,
            spectrum_advanced_at: Moment::default(),
            playlist_body_height: 0,
            wanted_cover: None,
            pending_crossfade: PendingCrossfade::default(),
            on_screen: OnScreen {
                progress_bar: None,
                clock: Presence::Hidden,
                sleep_label: Presence::Hidden,
                spectrum: Presence::Hidden,
            },
            screen_clear: ScreenClear::NotDue,
            outgoing_theme_background: None,
        }
    }
}

impl Motion {
    pub(in crate::shell) fn record_first_paint(&mut self, now: Moment) {
        if self.first_paint == Moment::default() {
            self.first_paint = now;
        }
    }

    pub(in crate::shell) fn advance_spectrum(
        &mut self,
        feed: SpectrumFeed,
        raw: &Spectrum,
    ) {
        if self.on_screen.spectrum == Presence::Hidden {
            return;
        }
        let elapsed = self.last_paint.elapsed_since(self.spectrum_advanced_at);
        self.spectrum_advanced_at = self.last_paint;
        let _ = match feed {
            SpectrumFeed::Live => self.spectrum_smoothing.smooth(raw, elapsed),
            SpectrumFeed::Silent => self.spectrum_smoothing.fade(elapsed),
        };
        self.spectrum_motion = self.spectrum_smoothing.motion();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::Moment;
    use widgets::{OnScreen, Presence, SPECTRUM_BANDS, SpectrumMotion};

    use crate::shell::motion::{Motion, SpectrumFeed};

    #[test]
    fn the_spectrum_moves_while_playing_and_settles_once_halted() {
        let mut motion = Motion {
            on_screen: OnScreen {
                progress_bar: None,
                clock: Presence::Hidden,
                sleep_label: Presence::Hidden,
                spectrum: Presence::Shown,
            },
            ..Motion::default()
        };
        motion.last_paint = Moment::new(Duration::from_millis(16));
        motion.advance_spectrum(SpectrumFeed::Live, &[1.0; SPECTRUM_BANDS]);
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Moving);

        for _ in 0..60 {
            motion.last_paint = Moment::new(
                motion.last_paint.since_epoch() + Duration::from_millis(16),
            );
            motion.advance_spectrum(SpectrumFeed::Silent, &[0.0; SPECTRUM_BANDS]);
        }
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Settled);
    }
}
