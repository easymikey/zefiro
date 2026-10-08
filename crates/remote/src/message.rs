use kernel::{
    cmd::{Cmds, RemoteCmd},
    domain::{
        revision::Revision,
        server::{Fetched, Listing, Page, RemoteError, ServerName, Session},
        track::CatalogRow,
    },
};

#[derive(Debug)]
pub enum RemoteMessage {
    Started,
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
        result: Result<Vec<CatalogRow>, RemoteError>,
        revision: Revision,
    },
    Fetched {
        revision: Revision,
        result: Result<Fetched, RemoteError>,
    },
    Elapsed(RemoteTimer),
    Found {
        server_name: ServerName,
        result: Result<Vec<CatalogRow>, RemoteError>,
        revision: Revision,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTimer {
    Search(Revision),
}

impl From<Cmds<RemoteCmd>> for RemoteMessage {
    fn from(cmds: Cmds<RemoteCmd>) -> Self {
        RemoteMessage::Cmds(cmds)
    }
}
