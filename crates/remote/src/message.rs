use std::path::PathBuf;

use kernel::{
    cmd::{Cmds, RemoteCmd},
    domain::{
        favorites::{Favorite, Favorites},
        io_error::IoError,
        revision::Revision,
        server::{
            Artwork,
            Fetched,
            Listing,
            Page,
            PlayReport,
            RemoteError,
            ServerName,
            ServerTrackId,
            Session,
        },
        track::CatalogRow,
    },
};

#[derive(Debug)]
pub enum RemoteMessage {
    Started,
    Restored(Result<Vec<PlayReport>, IoError>),
    Cmds(Cmds<RemoteCmd>),
    Connected {
        server_name: ServerName,
        result: Result<Session, RemoteError>,
        stored: Result<(), RemoteError>,
    },
    Listed {
        server_name: ServerName,
        listing: Listing,
        page: Page,
        result: Result<(Vec<CatalogRow>, Favorites), RemoteError>,
        revision: Revision,
    },
    Fetched {
        revision: Revision,
        result: Result<Fetched, RemoteError>,
    },
    Elapsed(RemoteTimer),
    Found {
        server_name: ServerName,
        result: Result<(Vec<CatalogRow>, Favorites), RemoteError>,
        revision: Revision,
    },
    Starred {
        server_name: ServerName,
        server_track_id: ServerTrackId,
        favorite: Favorite,
        result: Result<(), RemoteError>,
    },
    Forgotten(Result<(), RemoteError>),
    Reported {
        play_reports: Vec<PlayReport>,
        result: Result<(), RemoteError>,
    },
    Saved(Result<(), IoError>),
    Cover {
        artwork: Artwork,
        result: Result<PathBuf, RemoteError>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTimer {
    Search(Revision),
    Retry,
}

impl From<Cmds<RemoteCmd>> for RemoteMessage {
    fn from(cmds: Cmds<RemoteCmd>) -> Self {
        RemoteMessage::Cmds(cmds)
    }
}
