use std::{path::PathBuf, sync::Arc};

use kernel::{
    cmd::{Cmds, LibraryCmd},
    domain::{
        favorites::Favorites,
        history::HistoryEntry,
        io_error::IoError,
        overlay::Verdict,
        revision::Revision,
        track::Track,
    },
};

use crate::{
    cover::{CoverDecoded, CoverError},
    error::Error,
};

#[derive(Debug)]
pub enum LibraryMessage {
    Cmds(Cmds<LibraryCmd>),
    Changed(Result<(), IoError>),
    Elapsed(LibraryTimer),
    CoverDecoded {
        revision: Revision,
        decoded: Result<CoverDecoded, CoverError>,
    },
    Cached {
        music_dir: PathBuf,
        revision: Revision,
        tracks: Result<Vec<Arc<Track>>, Error>,
    },
    Scanned {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
        skipped: Option<Error>,
    },
    Tagged {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
        skipped: Option<Error>,
    },
    Listed {
        tracks: Vec<Arc<Track>>,
        revision: Revision,
        skipped: Option<Error>,
    },
    FavoritesLoaded(Favorites),
    Trashed(PathBuf),
    Checked {
        verdict: Verdict,
        revision: Revision,
    },
    HistoryLoaded {
        entries: Vec<HistoryEntry>,
        skipped: Option<Error>,
    },
    Error(Error),
}

impl From<Cmds<LibraryCmd>> for LibraryMessage {
    fn from(cmds: Cmds<LibraryCmd>) -> Self {
        LibraryMessage::Cmds(cmds)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryTimer {
    Debounce,
}
