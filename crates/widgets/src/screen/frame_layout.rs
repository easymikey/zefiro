use kernel::domain::{appearance::CoverMode, geometry::Cells};
use ratatui::layout::Rect;

use crate::{
    card::metrics::CardMetrics,
    overlay::modal::placement::OverlayAreas,
    playlist::pane::PlaylistAreas,
    screen::breakpoint::Breakpoint,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    pub screen: Rect,
    pub breakpoint: Breakpoint,
    pub content: Rect,
    pub header: Rect,
    pub card_metrics: Option<CardMetrics>,
    pub cover_area: Option<Rect>,
    pub playlist_pane: Rect,
    pub playlist_areas: Option<PlaylistAreas>,
    pub key_hints: Option<Rect>,
    pub search_bounds: Rect,
    pub overlay_areas: Option<OverlayAreas>,
    pub toast: Option<Rect>,
}

impl FrameLayout {
    #[must_use]
    pub fn empty(screen: Rect, breakpoint: Breakpoint) -> Self {
        Self {
            screen,
            breakpoint,
            content: Rect::default(),
            header: Rect::default(),
            card_metrics: None,
            cover_area: None,
            playlist_pane: Rect::default(),
            playlist_areas: None,
            key_hints: None,
            search_bounds: Rect::default(),
            overlay_areas: None,
            toast: None,
        }
    }

    #[must_use]
    pub fn playlist_body_height(&self) -> Cells {
        self.playlist_areas
            .map_or(Cells(0), |areas| Cells(areas.scroll_areas.content.height))
    }

    #[must_use]
    pub(crate) fn cover_exclusion(&self, cover_mode: CoverMode) -> Option<Rect> {
        match cover_mode {
            CoverMode::Vinyl | CoverMode::Plain => self.cover_area,
            CoverMode::Milkdrop | CoverMode::Off => None,
        }
    }
}
