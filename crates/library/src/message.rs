use std::{path::PathBuf, sync::Arc};

use kernel::{
    cmd::{Cmds, LibraryCmd},
    domain::{io_error::IoError, revision::Revision, track::Track},
    message::LibraryEvent,
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
        event: LibraryEvent,
        skipped: Option<Error>,
    },
    Tagged {
        event: LibraryEvent,
        skipped: Option<Error>,
    },
    Executed {
        event: LibraryEvent,
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
