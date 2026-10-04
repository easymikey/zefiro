use kernel::domain::time::Moment;
use ratatui::{layout::Rect, style::Color};
use runtime::repaint::FRAME_INTERVAL;
use widgets::{
    pixels::cover::gate::CrossfadeGate,
    repaint::{OnScreen, Presence},
    spectrum::SpectrumSmoothing,
};

pub(crate) struct Motion {
    pub(in crate::shell) first_paint: Option<Moment>,
    pub(in crate::shell) last_paint: Moment,
    pub(in crate::shell) area: Rect,
    pub(in crate::shell) spectrum_smoothing: SpectrumSmoothing,
    pub(in crate::shell) spectrum_advanced_at: Moment,
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

impl Default for Motion {
    fn default() -> Self {
        Self {
            first_paint: None,
            last_paint: Moment::default(),
            area: Rect::default(),
            spectrum_smoothing: SpectrumSmoothing::default(),
            spectrum_advanced_at: Moment::default(),
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

    pub(in crate::shell) fn next_frame(&self) -> Moment {
        Moment::new(self.last_paint.since_epoch() + FRAME_INTERVAL)
    }
}
