use std::sync::Arc;

use crate::domain::{PlaylistIndex, Track, TrackIndex, library::Library};

#[derive(Debug, Clone, Default)]
pub enum Loaded<T> {
    #[default]
    Loading,
    Ready(T),
}

impl<T> Loaded<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Loaded::Loading => None,
            Loaded::Ready(value) => Some(value),
        }
    }

    pub fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Loaded::Loading => None,
            Loaded::Ready(value) => Some(value),
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loaded::Loading)
    }
}

impl Loaded<Library> {
    #[must_use]
    pub(crate) fn view_track(&self, row: PlaylistIndex) -> Option<&Arc<Track>> {
        self.ready()?.view_track(row)
    }

    pub fn view_tracks(&self) -> impl Iterator<Item = (TrackIndex, &Arc<Track>)> {
        self.ready().into_iter().flat_map(Library::view_tracks)
    }
}
