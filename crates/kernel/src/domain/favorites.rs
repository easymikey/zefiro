use std::{collections::HashSet, sync::Arc};

use crate::domain::track::TrackSource;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Favorites(Arc<HashSet<TrackSource>>);

impl Favorites {
    #[must_use]
    pub fn is_favorite(&self, track_source: &TrackSource) -> bool {
        self.0.contains(track_source)
    }

    pub fn toggle(&mut self, track_source: TrackSource) {
        let set = Arc::make_mut(&mut self.0);
        if !set.remove(&track_source) {
            set.insert(track_source);
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TrackSource> {
        self.0.iter()
    }
}

impl FromIterator<TrackSource> for Favorites {
    fn from_iter<I: IntoIterator<Item = TrackSource>>(tracks: I) -> Self {
        Self(Arc::new(tracks.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{favorites::Favorites, track::TrackSource};

    fn path(text: &str) -> TrackSource {
        TrackSource::Local(text.into())
    }

    #[test]
    fn toggle_favorites_a_path_then_toggle_unfavorites_it() {
        let mut favorites = Favorites::default();
        assert!(!favorites.is_favorite(&path("/a.flac")));

        favorites.toggle(path("/a.flac"));
        assert!(favorites.is_favorite(&path("/a.flac")));

        favorites.toggle(path("/a.flac"));
        assert!(!favorites.is_favorite(&path("/a.flac")));
    }

    #[test]
    fn is_empty_tracks_membership() {
        let mut favorites = Favorites::default();
        assert!(favorites.is_empty());

        favorites.toggle(path("/a.flac"));
        assert!(!favorites.is_empty());
    }

    #[test]
    fn collecting_paths_and_iterating_round_trip() {
        let favorites: Favorites = [path("/a.flac")].into_iter().collect();

        assert!(favorites.is_favorite(&path("/a.flac")));
        assert_eq!(favorites.iter().collect::<Vec<_>>(), [&path("/a.flac")]);
    }
}
