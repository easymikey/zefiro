use crate::domain::{
    cursor::Cursor,
    overlay::ServerQuery,
    playlist::PlaylistSource,
    server::{AlbumOrder, Listing, Page, ServerName},
    track::CatalogRow,
};

pub const PAGE_LEAD: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CatalogName {
    #[default]
    Local,
    Server(ServerName),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paging {
    Next(Page),
    Queued(Page),
    Loading(Page),
    Complete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BrowseLevel {
    pub listing: Listing,
    pub catalog_rows: Vec<CatalogRow>,
    pub cursor: Cursor,
    pub paging: Paging,
    pub server_query: Option<ServerQuery>,
}

impl BrowseLevel {
    #[must_use]
    pub fn new(listing: Listing) -> Self {
        Self {
            listing,
            catalog_rows: Vec::new(),
            cursor: Cursor::default(),
            paging: Paging::Next(Page::default()),
            server_query: None,
        }
    }

    #[must_use]
    pub fn query(&self) -> Option<&ServerQuery> {
        self.server_query
            .as_ref()
            .filter(|server_query| !server_query.input.is_empty())
    }

    #[must_use]
    pub fn rows(&self) -> &[CatalogRow] {
        self.query().map_or(&self.catalog_rows, |server_query| {
            &server_query.catalog_rows
        })
    }

    pub(crate) fn queue(&mut self) {
        match self.paging {
            Paging::Next(page) => self.paging = Paging::Queued(page),
            Paging::Queued(_) | Paging::Loading(_) | Paging::Complete => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub server_name: ServerName,
    pub albums_level: BrowseLevel,
    pub album_level: Option<BrowseLevel>,
    pub browse_levels: [BrowseLevel; 2],
}

impl Catalog {
    #[must_use]
    pub fn new(server_name: ServerName) -> Self {
        Self {
            server_name,
            albums_level: BrowseLevel::new(Listing::Songs),
            album_level: None,
            browse_levels: [
                BrowseLevel::new(Listing::Albums(AlbumOrder::Newest)),
                BrowseLevel::new(Listing::Playlists),
            ],
        }
    }

    #[must_use]
    pub fn playlist_source(&self) -> PlaylistSource {
        let server_name = self.server_name.clone();
        match (&self.album_level, &self.albums_level.listing) {
            (None, Listing::Songs) if self.albums_level.query().is_none() => {
                PlaylistSource::Songs(server_name)
            }
            (
                Some(_),
                Listing::Songs
                | Listing::Albums(_)
                | Listing::Album(_)
                | Listing::Playlists
                | Listing::Playlist(_),
            )
            | (
                None,
                Listing::Songs
                | Listing::Albums(_)
                | Listing::Album(_)
                | Listing::Playlists
                | Listing::Playlist(_),
            ) => PlaylistSource::Server(server_name),
        }
    }

    pub fn level(&mut self) -> &mut BrowseLevel {
        self.album_level.as_mut().unwrap_or(&mut self.albums_level)
    }

    pub(crate) fn levels(&mut self) -> impl Iterator<Item = &mut BrowseLevel> {
        let Self {
            server_name: _,
            albums_level,
            album_level,
            browse_levels,
        } = self;
        album_level
            .iter_mut()
            .chain([albums_level])
            .chain(browse_levels)
    }
}
