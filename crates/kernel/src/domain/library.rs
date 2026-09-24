use std::{borrow::Borrow, sync::Arc};

use strum::EnumIter;

use crate::domain::{Favorites, PlaylistIndex, Track, TrackIndex};

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub all: Vec<Arc<Track>>,
    pub view: Vec<TrackIndex>,
}

impl Library {
    #[must_use]
    pub(crate) fn view_track(&self, row: PlaylistIndex) -> Option<&Arc<Track>> {
        let index = self.view.get(row.get())?;
        self.all.get(index.get())
    }

    pub fn view_tracks(&self) -> impl Iterator<Item = (TrackIndex, &Arc<Track>)> {
        self.view.iter().filter_map(move |&index| {
            self.all.get(index.get()).map(|track| (index, track))
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

pub fn sort_indices<T: Borrow<Track>>(
    tracks: &[T],
    key: SortKey,
    favorites: &Favorites,
) -> Vec<usize> {
    match key {
        SortKey::Added => (0..tracks.len()).collect(),
        SortKey::Favorites => {
            let mut indices: Vec<usize> = (0..tracks.len()).collect();
            indices.sort_by_key(|&index| {
                !tracks
                    .get(index)
                    .map(Borrow::borrow)
                    .is_some_and(|found| favorites.is_favorite(found.path()))
            });
            indices
        }
        SortKey::Artist => sort_by_artist(tracks),
        SortKey::Album => sort_by_key(tracks, |found| &found.tags().album),
        SortKey::Year => sort_by_key(tracks, |found| &found.tags().date),
    }
}

fn sort_by_key<T: Borrow<Track>>(
    tracks: &[T],
    field: impl Fn(&Track) -> &Option<String>,
) -> Vec<usize> {
    let mut keyed: Vec<(String, usize)> = tracks
        .iter()
        .enumerate()
        .map(|(index, item)| (lower(field(item.borrow())), index))
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, index)| index).collect()
}

fn sort_by_artist<T: Borrow<Track>>(tracks: &[T]) -> Vec<usize> {
    let mut keyed: Vec<(String, String, usize)> = tracks
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let found = item.borrow();
            (
                lower(&found.tags().artist),
                lower(&found.tags().album),
                index,
            )
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, _, index)| index).collect()
}
