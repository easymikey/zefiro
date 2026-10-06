use crate::domain::{
    time::Moment,
    track::{Track, TrackRef},
};

pub(crate) const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub track: TrackRef,
    pub title: String,
    pub artist: Option<String>,
    pub at: Moment,
}

impl HistoryEntry {
    #[must_use]
    pub fn from_track(track: &Track, at: Moment) -> Self {
        Self {
            track: track.source().clone(),
            title: track.song_title().to_owned(),
            artist: track.tags().artist.clone(),
            at,
        }
    }
}
