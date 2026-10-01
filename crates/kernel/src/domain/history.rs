use std::path::PathBuf;

use crate::domain::{Moment, Track};

pub const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub at: Moment,
}

impl HistoryEntry {
    #[must_use]
    pub fn from_track(track: &Track, at: Moment) -> Self {
        Self {
            path: track.path().to_path_buf(),
            title: track.song_title(),
            artist: track.tags().artist.clone(),
            at,
        }
    }
}
