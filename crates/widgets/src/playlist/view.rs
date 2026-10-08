use kernel::domain::{
    catalog::{BrowseLevel, Catalog},
    favorites::Favorites,
    index::ViewIndex,
    playlist::Playlist,
    server::Server,
    track::{Track, TrackSource},
};

use crate::{primitive::track_row::Playing, status_line::StatusLineView};

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
    pub(crate) catalog_view: Option<CatalogView<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CatalogView<'a> {
    pub(crate) catalog: &'a Catalog,
    pub(crate) server: &'a Server,
    pub(crate) favorites: &'a Favorites,
    pub(crate) playing_track_source: Option<&'a TrackSource>,
}

impl<'a> CatalogView<'a> {
    #[must_use]
    pub(crate) fn level(self) -> &'a BrowseLevel {
        self.catalog
            .album_level
            .as_ref()
            .unwrap_or(&self.catalog.albums_level)
    }

    pub(crate) fn playing(self, track: &Track) -> Playing {
        if self.playing_track_source == Some(track.source()) {
            Playing::Yes
        } else {
            Playing::No
        }
    }
}
