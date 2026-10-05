use kernel::domain::{
    favorites::Favorites,
    index::ViewIndex,
    playlist::Playlist,
    track::TrackRef,
};

use crate::status_line::StatusLineView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryLoad {
    Loading,
    Ready,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaylistView<'a> {
    pub(crate) playlist: &'a Playlist,
    pub(crate) queue: &'a [TrackRef],
    pub(crate) favorites: &'a Favorites,
    pub(crate) browse_selected: usize,
    pub(crate) playing: Option<ViewIndex>,
    pub(crate) library_loading: LibraryLoad,
    pub(crate) status: StatusLineView<'a>,
}
