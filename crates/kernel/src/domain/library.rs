use std::sync::Arc;

use strum::EnumIter;

use crate::domain::{
    favorites::Favorites,
    index::{TrackIndex, ViewIndex},
    track::Track,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub tracks: Vec<Arc<Track>>,
    pub track_indexes: Vec<TrackIndex>,
}

impl Library {
    #[must_use]
    pub fn view_track(&self, index: ViewIndex) -> Option<&Arc<Track>> {
        let index = self.track_indexes.get(index.get())?;
        self.tracks.get(index.get())
    }

    pub fn view_tracks(&self) -> impl Iterator<Item = (TrackIndex, &Arc<Track>)> {
        self.track_indexes.iter().filter_map(move |&index| {
            self.tracks.get(index.get()).map(|track| (index, track))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumIter)]
pub enum SortKey {
    Artist,
    Album,
    Year,
    #[default]
    Added,
    Favorites,
}

fn lower(text: &Option<String>) -> String {
    text.as_deref().unwrap_or("").to_lowercase()
}

pub fn sort_indices(
    tracks: &[Arc<Track>],
    key: SortKey,
    favorites: &Favorites,
) -> Vec<TrackIndex> {
    match key {
        SortKey::Added => (0..tracks.len()).map(TrackIndex::new).collect(),
        SortKey::Favorites => {
            sorted(tracks, |found| !favorites.is_favorite(found.source()))
        }
        SortKey::Artist => sorted(tracks, |found| {
            (lower(&found.tags().artist), lower(&found.tags().album))
        }),
        SortKey::Album => sorted(tracks, |found| lower(&found.tags().album)),
        SortKey::Year => sorted(tracks, |found| lower(&found.tags().date)),
    }
}

fn sorted<K: Ord>(
    tracks: &[Arc<Track>],
    rank: impl Fn(&Track) -> K,
) -> Vec<TrackIndex> {
    let mut track_indexes: Vec<TrackIndex> =
        (0..tracks.len()).map(TrackIndex::new).collect();
    track_indexes
        .sort_by_cached_key(|index| tracks.get(index.get()).map(|found| rank(found)));
    track_indexes
}
