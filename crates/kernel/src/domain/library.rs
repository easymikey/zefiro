use std::sync::Arc;

use strum::EnumIter;

use crate::domain::{
    favorites::Favorites,
    index::{TrackIndex, ViewIndex},
    track::Track,
};

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub tracks: Vec<Arc<Track>>,
    pub view: Vec<TrackIndex>,
}

impl Library {
    #[must_use]
    pub fn view_track(&self, row: ViewIndex) -> Option<&Arc<Track>> {
        let index = self.view.get(row.get())?;
        self.tracks.get(index.get())
    }

    pub fn view_tracks(&self) -> impl Iterator<Item = (TrackIndex, &Arc<Track>)> {
        self.view.iter().filter_map(move |&index| {
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
            let mut indices: Vec<TrackIndex> =
                (0..tracks.len()).map(TrackIndex::new).collect();
            indices.sort_by_key(|&index| {
                !tracks
                    .get(index.get())
                    .is_some_and(|found| favorites.is_favorite(found.source()))
            });
            indices
        }
        SortKey::Artist => sort_by_artist(tracks),
        SortKey::Album => sort_by_key(tracks, |found| &found.tags().album),
        SortKey::Year => sort_by_key(tracks, |found| &found.tags().date),
    }
}

fn sort_by_key(
    tracks: &[Arc<Track>],
    field: impl Fn(&Track) -> &Option<String>,
) -> Vec<TrackIndex> {
    let mut keyed: Vec<(String, TrackIndex)> = tracks
        .iter()
        .enumerate()
        .map(|(index, found)| (lower(field(found)), TrackIndex::new(index)))
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, index)| index).collect()
}

fn sort_by_artist(tracks: &[Arc<Track>]) -> Vec<TrackIndex> {
    let mut keyed: Vec<(String, String, TrackIndex)> = tracks
        .iter()
        .enumerate()
        .map(|(index, found)| {
            (
                lower(&found.tags().artist),
                lower(&found.tags().album),
                TrackIndex::new(index),
            )
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, _, index)| index).collect()
}
