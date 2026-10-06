use kernel::domain::{
    favorites::Favorites,
    index::ViewIndex,
    playlist::Playlist,
    track::TrackSource,
};

use crate::status_line::StatusLineView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryStatus {
    Loading,
    Ready,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaylistView<'a> {
    pub(crate) playlist: &'a Playlist,
    pub(crate) queue: &'a [TrackSource],
    pub(crate) favorites: &'a Favorites,
    pub(crate) selected: ViewIndex,
    pub(crate) playing_index: Option<ViewIndex>,
    pub(crate) library_status: LibraryStatus,
    pub(crate) status_line_view: StatusLineView<'a>,
}
