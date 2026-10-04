use std::sync::Arc;

use strum::{EnumDiscriminants, EnumIter, IntoStaticStr};

use crate::domain::{
    cursor_over::CursorOver,
    index::ViewIndex,
    playlist::PlaylistNameError,
    setting_row::SettingRow,
    time::TimecodeError,
    track::{Track, TrackRef},
    workspace::{SaveLine, SavePhase},
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr, EnumDiscriminants)]
#[strum(serialize_all = "snake_case")]
#[strum_discriminants(
    name(OverlayName),
    derive(IntoStaticStr, EnumIter),
    strum(serialize_all = "snake_case")
)]
pub enum Overlay {
    Help,
    Search(CursorOver<SearchQuery>),
    SavePlaylist {
        typed: TextEntry,
        error: Option<PlaylistNameError>,
    },
    History(CursorOver<()>),
    Settings(SettingRow),
    ConfirmDelete(DeleteCandidate),
    JumpToTime(JumpDigits),
    TrackDetails(Arc<Track>),
    MusicDir {
        typed: TextEntry,
        error: Option<MusicDirError>,
    },
}

impl Overlay {
    #[must_use]
    pub(crate) fn captures_text(&self) -> bool {
        match self {
            Overlay::Search(_)
            | Overlay::SavePlaylist { .. }
            | Overlay::MusicDir { .. } => true,
            Overlay::Help
            | Overlay::History(_)
            | Overlay::Settings(..)
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_) => false,
        }
    }

    #[must_use]
    pub fn save_line(&self) -> Option<SaveLine> {
        match self {
            Overlay::SavePlaylist { typed, error: None } => Some(SaveLine {
                text: format!("Save playlist: {}", typed.input),
                phase: SavePhase::Prompt,
            }),
            Overlay::SavePlaylist {
                error: Some(reason),
                ..
            } => Some(SaveLine {
                text: reason.to_string(),
                phase: SavePhase::Failed,
            }),
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::History(_)
            | Overlay::Settings(..)
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::MusicDir { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEntry {
    pub input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MusicDirError {
    #[error("enter a folder path")]
    Empty,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub input: String,
    pub matches: Vec<ViewIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteCandidate {
    pub source: TrackRef,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JumpDigits {
    pub input: String,
    pub error: Option<TimecodeError>,
}

impl JumpDigits {
    pub(crate) const SEPARATOR: char = ':';
    pub(crate) const MAX_LEN: usize = 8;
}
