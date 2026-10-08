use crate::domain::{
    cursor::Cursor,
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
    Loading(Page),
    Complete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BrowseLevel {
    pub listing: Listing,
    pub catalog_rows: Vec<CatalogRow>,
    pub cursor: Cursor,
    pub paging: Paging,
}

impl BrowseLevel {
    #[must_use]
    pub fn new(listing: Listing) -> Self {
        Self {
            listing,
            catalog_rows: Vec::new(),
            cursor: Cursor::default(),
            paging: Paging::Next(Page::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub server_name: ServerName,
    pub albums_level: BrowseLevel,
    pub album_level: Option<BrowseLevel>,
}

impl Catalog {
    #[must_use]
    pub fn new(server_name: ServerName) -> Self {
        Self {
            server_name,
            albums_level: BrowseLevel::new(Listing::Albums(AlbumOrder::Newest)),
            album_level: None,
        }
    }
}
