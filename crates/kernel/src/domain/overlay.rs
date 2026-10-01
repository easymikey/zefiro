use std::sync::Arc;

use strum::{EnumDiscriminants, EnumIter, IntoStaticStr};

use crate::domain::{
    CursorOver,
    PlaylistIndex,
    SettingRow,
    TimecodeError,
    Track,
    playlist::PlaylistNameError,
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
    pub fn captures_text(&self) -> bool {
        match self {
            Overlay::Search(_)
            | Overlay::SavePlaylist { .. }
            | Overlay::MusicDir { .. } => true,
            Overlay::Help
            | Overlay::History(_)
            | Overlay::Settings { .. }
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_) => false,
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
    pub const MAX_LEN: usize = 8;
}
