use std::{collections::HashSet, ops::Not, sync::Arc};

use crate::domain::track::TrackSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Favorite {
    Yes,
    No,
}

impl Not for Favorite {
    type Output = Self;

    fn not(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Favorites(Arc<HashSet<TrackSource>>);

impl Favorites {
    #[must_use]
    pub fn is_favorite(&self, track_source: &TrackSource) -> bool {
        self.0.contains(track_source)
    }

    #[must_use]
    pub fn favorite(&self, track_source: &TrackSource) -> Favorite {
        if self.is_favorite(track_source) {
            Favorite::Yes
        } else {
            Favorite::No
        }
    }

    pub fn set(&mut self, track_source: TrackSource, favorite: Favorite) {
        let set = Arc::make_mut(&mut self.0);
        match favorite {
            Favorite::Yes => {
                set.insert(track_source);
            }
            Favorite::No => {
                set.remove(&track_source);
            }
        }
    }

    pub fn toggle(&mut self, track_source: TrackSource) {
        let favorite = !self.favorite(&track_source);
        self.set(track_source, favorite);
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
    fn toggling_a_path_twice_leaves_no_favorites() {
        let mut favorites = Favorites::default();
        assert!(favorites.iter().next().is_none());

        favorites.toggle(path("/a.flac"));
        assert!(favorites.iter().next().is_some());

        favorites.toggle(path("/a.flac"));
        assert_eq!(favorites, Favorites::default());
    }

    #[test]
    fn collecting_paths_and_iterating_round_trip() {
        let favorites: Favorites = [path("/a.flac")].into_iter().collect();

        assert!(favorites.is_favorite(&path("/a.flac")));
        assert_eq!(favorites.iter().collect::<Vec<_>>(), [&path("/a.flac")]);
    }
}
