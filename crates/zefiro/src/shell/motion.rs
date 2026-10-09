use std::time::Duration;

use kernel::domain::time::Moment;
use ratatui::layout::Rect;
use runtime::repaint::FRAME_INTERVAL;
use widgets::{
    animation::catalogue::PaintedCell,
    repaint::{OnScreen, Presence},
    spectrum::SpectrumSmoothing,
};

pub(crate) struct Motion {
    pub(in crate::shell) paint_clock: PaintClock,
    pub(in crate::shell) area: Rect,
    pub(in crate::shell) spectrum_smoothing: SpectrumSmoothing,
    pub(in crate::shell) spectrum_advanced_at: Moment,
    pub(in crate::shell) on_screen: OnScreen,
    pub(in crate::shell) screen_clear: ScreenClear,
    pub(in crate::shell) painted_cells: Vec<PaintedCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PaintClock {
    NotPainted,
    Painted { first: Moment, last: Moment },
}

impl PaintClock {
    pub(in crate::shell) fn record(self, now: Moment) -> Self {
        match self {
            Self::NotPainted => Self::Painted {
                first: now,
                last: now,
            },
            Self::Painted { first, .. } => Self::Painted { first, last: now },
        }
    }

    pub(in crate::shell) fn elapsed(self, now: Moment) -> Duration {
        match self {
            Self::NotPainted => Duration::ZERO,
            Self::Painted { first, .. } => now.elapsed_since(first),
        }
    }

    fn last(self) -> Moment {
        match self {
            Self::NotPainted => Moment::default(),
            Self::Painted { last, .. } => last,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ScreenClear {
    NotDue,
    Due,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            paint_clock: PaintClock::NotPainted,
            area: Rect::default(),
            spectrum_smoothing: SpectrumSmoothing::default(),
            spectrum_advanced_at: Moment::default(),
            on_screen: OnScreen {
                progress_bar_width: None,
                clock: Presence::Hidden,
                sleep_label: Presence::Hidden,
                spectrum: Presence::Hidden,
                spinner: Presence::Hidden,
            },
            screen_clear: ScreenClear::NotDue,
            painted_cells: Vec::new(),
        }
    }
}

impl Motion {
    pub(in crate::shell) fn next_frame(&self) -> Moment {
        Moment::new(self.paint_clock.last().since_epoch() + FRAME_INTERVAL)
    }
}
