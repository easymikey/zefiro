use std::sync::Arc;

use strum::{EnumDiscriminants, EnumIter, IntoStaticStr};

use crate::domain::{
    cursor_over::CursorOver,
    index::ViewIndex,
    playlist::PlaylistFileNameError,
    setting_row::SettingRow,
    time::TimecodeError,
    track::{Track, TrackSource},
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
    SavePlaylist(TextEntry<PlaylistFileNameError>),
    History(CursorOver<()>),
    Settings(SettingRow),
    ConfirmTrash(TrashCandidate),
    JumpToTime(TextEntry<TimecodeError>),
    TrackDetails(Arc<Track>),
    MusicDir(TextEntry<MusicDirError>),
}

impl Overlay {
    #[must_use]
    pub(crate) fn captures_text(&self) -> bool {
        match self {
            Overlay::Search(_) | Overlay::SavePlaylist(_) | Overlay::MusicDir(_) => {
                true
            }
            Overlay::Help
            | Overlay::History(_)
            | Overlay::Settings(..)
            | Overlay::ConfirmTrash(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEntry<E> {
    pub input: String,
    pub error: Option<E>,
}

impl<E> Default for TextEntry<E> {
    fn default() -> Self {
        Self {
            input: String::new(),
            error: None,
        }
    }
}

pub trait Accepts {
    const MAX_LEN: usize;

    #[must_use]
    fn accepts(character: char) -> bool;
}

impl Accepts for PlaylistFileNameError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(_character: char) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MusicDirError {
    #[error("enter a folder path")]
    Empty,
}

impl Accepts for MusicDirError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(_character: char) -> bool {
        true
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub input: String,
    pub matches: Vec<ViewIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashCandidate {
    pub source: TrackSource,
    pub title: String,
    pub artist: String,
}
