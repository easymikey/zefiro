use std::sync::Arc;

use strum::IntoStaticStr;

use crate::domain::{
    CursorOver,
    PlaylistIndex,
    SettingRow,
    TimecodeError,
    Track,
    playlist::PlaylistNameError,
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Overlay {
    Help,
    Search(CursorOver<SearchQuery>),
    SavePlaylist {
        typed: TextEntry,
        error: Option<PlaylistNameError>,
    },
    History(CursorOver<()>),
    Settings {
        selected: SettingRow,
    },
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
    pub fn captures_text(&self) -> TextCapture {
        match self {
            Overlay::Search(_)
            | Overlay::SavePlaylist { .. }
            | Overlay::MusicDir { .. } => TextCapture::Typing,
            Overlay::Help
            | Overlay::History(_)
            | Overlay::Settings { .. }
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_) => TextCapture::Chording,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum OverlayName {
    Help,
    Search,
    SavePlaylist,
    History,
    Settings,
    ConfirmDelete,
    TrackDetails,
    JumpToTime,
    MusicDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCapture {
    Typing,
    Chording,
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
    pub matches: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteCandidate {
    pub track: PlaylistIndex,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JumpDigits {
    pub input: String,
    pub error: Option<TimecodeError>,
}

impl JumpDigits {
    pub const SEPARATOR: char = ':';
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JumpInputLimits {
    pub max_len: usize,
}

impl Default for JumpInputLimits {
    fn default() -> Self {
        Self { max_len: 8 }
    }
}
