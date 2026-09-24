use std::path::PathBuf;

use kernel::domain::Revision;

use crate::library::watch::{Burst, LibraryWatch, Registered, WatchIo};

pub(crate) fn rescan(
    registered: Registered,
    root: PathBuf,
    revision: Revision,
) -> (LibraryWatch, WatchIo) {
    let (registered, io) = repoint(registered, root, revision);
    (
        LibraryWatch {
            registered,
            last_scan: revision,
        },
        io,
    )
}

fn repoint(
    registered: Registered,
    target: PathBuf,
    revision: Revision,
) -> (Registered, WatchIo) {
    match registered {
        Registered::Unrooted => (
            Registered::On {
                root: target.clone(),
                burst: Burst::Quiet,
            },
            WatchIo::RegisterAndRescan {
                root: target,
                revision,
            },
        ),
        Registered::On { root, .. } if root == target => (
            Registered::On {
                root,
                burst: Burst::Quiet,
            },
            WatchIo::Rescan {
                root: target,
                revision,
            },
        ),
        Registered::On { root, .. } => (
            Registered::On {
                root: target.clone(),
                burst: Burst::Quiet,
            },
            WatchIo::Move {
                from: root,
                to: target,
                revision,
            },
        ),
    }
}
