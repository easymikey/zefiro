use std::sync::Arc;

use kernel::domain::{
    appearance::CoverMode,
    cursor_over::CursorOver,
    geometry::Cells,
    overlay::{MusicDirError, SearchQuery, ServerPrompt, ServerQuery, TextEntry},
    playlist::PlaylistFileNameError,
    server::ServerName,
    setting_row::SettingRow,
    time::TimecodeError,
    track::Track,
};
use ratatui::layout::Rect;

use crate::{
    card::metrics::CardMetrics,
    overlay::{
        help::HelpColumns,
        history::HistoryMeasures,
        modal::placement::OverlayAreas,
        servers::ServersTable,
        settings::SettingsTable,
        track_details::TrackDetailsRow,
    },
    playlist::row::PlaylistAreas,
    screen::breakpoint::Breakpoint,
    toast::Placement,
};

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayContent<'a> {
    Help(HelpColumns),
    Search(&'a CursorOver<SearchQuery>, String),
    ServerSearch(&'a CursorOver<ServerQuery>),
    SavePlaylist(&'a TextEntry<PlaylistFileNameError>),
    History(&'a CursorOver<()>, HistoryMeasures),
    Settings(SettingRow, SettingsTable),
    ConfirmTrash(&'a Arc<Track>),
    JumpToTime(&'a TextEntry<TimecodeError>),
    TrackDetails(Vec<TrackDetailsRow<'a>>),
    MusicDir(&'a TextEntry<MusicDirError>),
    AddServer(&'a ServerPrompt),
    Servers(&'a CursorOver<()>, ServersTable<'a>),
    ConfirmRemove(&'a ServerName),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameLayout<'a> {
    pub screen: Rect,
    pub breakpoint: Breakpoint,
    pub content: Rect,
    pub header: Rect,
    pub card_metrics: Option<CardMetrics>,
    pub progress_bar_width: Cells,
    pub remaining_label: String,
    pub cover_area: Option<Rect>,
    pub playlist_pane: Rect,
    pub playlist_areas: Option<PlaylistAreas>,
    pub key_hints: Option<Rect>,
    pub search_bounds: Rect,
    pub overlay_areas: Option<OverlayAreas>,
    pub overlay_content: Option<OverlayContent<'a>>,
    pub toast_placement: Option<Placement<'a>>,
}

impl FrameLayout<'_> {
    #[must_use]
    pub fn empty(screen: Rect, breakpoint: Breakpoint) -> Self {
        Self {
            screen,
            breakpoint,
            content: Rect::default(),
            header: Rect::default(),
            card_metrics: None,
            progress_bar_width: Cells(0),
            remaining_label: String::new(),
            cover_area: None,
            playlist_pane: Rect::default(),
            playlist_areas: None,
            key_hints: None,
            search_bounds: Rect::default(),
            overlay_areas: None,
            overlay_content: None,
            toast_placement: None,
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
