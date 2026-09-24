use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Favorites(Arc<HashSet<PathBuf>>);

impl Favorites {
    #[must_use]
    pub fn is_favorite(&self, path: &Path) -> bool {
        self.0.contains(path)
    }

    pub fn toggle(&mut self, path: PathBuf) {
        let set = Arc::make_mut(&mut self.0);
        if !set.remove(&path) {
            set.insert(path);
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn into_inner(self) -> Arc<HashSet<PathBuf>> {
        self.0
    }
}

impl From<HashSet<PathBuf>> for Favorites {
    fn from(paths: HashSet<PathBuf>) -> Self {
        Self(Arc::new(paths))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use crate::domain::favorites::Favorites;

    fn path(text: &str) -> PathBuf {
        PathBuf::from(text)
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
    fn from_hash_set_and_into_inner_round_trip() {
        let mut set = HashSet::new();
        set.insert(path("/a.flac"));

        let favorites = Favorites::from(set.clone());
        assert!(favorites.is_favorite(&path("/a.flac")));
        assert_eq!(*favorites.into_inner(), set);
    }
}
