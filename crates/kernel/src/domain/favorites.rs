use std::{collections::HashSet, sync::Arc};

use crate::domain::TrackRef;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Favorites(Arc<HashSet<TrackRef>>);

impl Favorites {
    #[must_use]
    pub fn is_favorite(&self, track: &TrackRef) -> bool {
        self.0.contains(track)
    }

    pub fn toggle(&mut self, track: TrackRef) {
        let set = Arc::make_mut(&mut self.0);
        if !set.remove(&track) {
            set.insert(track);
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TrackRef> {
        self.0.iter()
    }
}

impl FromIterator<TrackRef> for Favorites {
    fn from_iter<I: IntoIterator<Item = TrackRef>>(tracks: I) -> Self {
        Self(Arc::new(tracks.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{TrackRef, favorites::Favorites};

    fn path(text: &str) -> TrackRef {
        TrackRef::Local(text.into())
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
