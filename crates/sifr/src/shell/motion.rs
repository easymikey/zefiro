use std::path::PathBuf;

use kernel::{Moment, Player, domain::geometry::Cells};
use library::CoverJob;
use ratatui::{layout::Rect, style::Color};
use widgets::{
    CrossfadePermit,
    OnScreen,
    Presence,
    Spectrum,
    SpectrumMotion,
    SpectrumSmoothing,
};

use crate::shell::cover_crossfade::CrossfadeGate;

pub(crate) struct Motion {
    pub(in crate::shell) first_paint: Option<Moment>,
    pub(in crate::shell) last_paint: Moment,
    pub(in crate::shell) area: Rect,
    pub(in crate::shell) spectrum_smoothing: SpectrumSmoothing,
    pub(in crate::shell) spectrum_motion: SpectrumMotion,
    pub(in crate::shell) spectrum_advanced_at: Moment,
    pub(in crate::shell) playlist_body_height: Cells,
    pub(in crate::shell) wanted_cover: Option<PathBuf>,
    pub(in crate::shell) crossfade_gate: CrossfadeGate,
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

pub(crate) struct FrameAdvance {
    pub(crate) cover: Option<CoverJob>,
    pub(crate) visible_rows: Option<Cells>,
    pub(crate) crossfade: CrossfadePermit,
    pub(crate) screen_clear: ScreenClear,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            first_paint: None,
            last_paint: Moment::default(),
            area: Rect::default(),
            spectrum_smoothing: SpectrumSmoothing::default(),
            spectrum_motion: SpectrumMotion::Settled,
            spectrum_advanced_at: Moment::default(),
            playlist_body_height: Cells(0),
            wanted_cover: None,
            crossfade_gate: CrossfadeGate::default(),
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
        self.first_paint = self.first_paint.or(Some(now));
    }

    pub(in crate::shell) fn advance_spectrum(
        &mut self,
        feed: SpectrumFeed,
        raw: &Spectrum,
    ) -> Spectrum {
        if self.on_screen.spectrum == Presence::Hidden {
            return *self.spectrum_smoothing.bands();
        }
        let elapsed = self.last_paint.elapsed_since(self.spectrum_advanced_at);
        self.spectrum_advanced_at = self.last_paint;
        let smoothed = match feed {
            SpectrumFeed::Live => self.spectrum_smoothing.smooth(raw, elapsed),
            SpectrumFeed::Silent => self.spectrum_smoothing.fade(elapsed),
        };
        self.spectrum_motion = self.spectrum_smoothing.motion();
        smoothed
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
        let lifted =
            motion.advance_spectrum(SpectrumFeed::Live, &[1.0; SPECTRUM_BANDS]);
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Moving);
        assert!(lifted.iter().all(|&band| band > 0.0));

        for _ in 0..60 {
            motion.last_paint = Moment::new(
                motion.last_paint.since_epoch() + Duration::from_millis(16),
            );
            let faded =
                motion.advance_spectrum(SpectrumFeed::Silent, &[0.0; SPECTRUM_BANDS]);
            assert!(faded.iter().all(|&band| band < 1.0));
        }
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Settled);
    }
}
