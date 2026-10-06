use crate::domain::{
    time::Moment,
    track::{Track, TrackSource},
};

pub(crate) const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub track_source: TrackSource,
    pub title: String,
    pub artist: Option<String>,
    pub played_at: Moment,
}

impl HistoryEntry {
    #[must_use]
    pub fn from_track(track: &Track, played_at: Moment) -> Self {
        Self {
            track_source: track.source().clone(),
            title: track.title().to_owned(),
            artist: track.tags().artist.clone(),
            played_at,
        }
    }
}
