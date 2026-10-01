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

    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.0.iter()
    }
}

impl FromIterator<PathBuf> for Favorites {
    fn from_iter<I: IntoIterator<Item = PathBuf>>(paths: I) -> Self {
        Self(Arc::new(paths.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    fn collecting_paths_and_iterating_round_trip() {
        let favorites: Favorites = [path("/a.flac")].into_iter().collect();

        assert!(favorites.is_favorite(&path("/a.flac")));
        assert_eq!(favorites.iter().collect::<Vec<_>>(), [&path("/a.flac")]);
    }
}
