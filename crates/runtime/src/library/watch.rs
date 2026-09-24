use std::path::PathBuf;

use kernel::{
    domain::Revision,
    update::{Machine, Rejected},
};

use crate::library::repoint;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LibraryWatch {
    pub(crate) registered: Registered,
    pub(crate) last_scan: Revision,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Registered {
    #[default]
    Unrooted,
    On {
        root: PathBuf,
        burst: Burst,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Burst {
    Quiet,
    Armed,
}

#[derive(Debug)]
pub(crate) enum LibraryWatchMessage {
    Rescan { root: PathBuf, revision: Revision },
    FilesystemChange(Result<(), notify::Error>),
    DebounceElapsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryWatchRejection {
    Unwatched,
    Settled,
}

#[derive(Debug, Default)]
pub(crate) enum WatchIo {
    Move {
        from: PathBuf,
        to: PathBuf,
        revision: Revision,
    },
    ArmDebounce,
    Rescan {
        root: PathBuf,
        revision: Revision,
    },
    RegisterAndRescan {
        root: PathBuf,
        revision: Revision,
    },
    Report(notify::Error),
    #[default]
    Nothing,
}

type Step = Result<(LibraryWatch, WatchIo), Rejected<LibraryWatch>>;
pub(crate) type Row =
    Result<(Registered, WatchIo), (Registered, LibraryWatchRejection)>;

impl Machine for LibraryWatch {
    type Message = LibraryWatchMessage;
    type Rejection = LibraryWatchRejection;
    type Effect = WatchIo;

    fn transition(self, message: LibraryWatchMessage) -> Step {
        let LibraryWatch {
            registered,
            last_scan,
        } = self;
        match message {
            LibraryWatchMessage::Rescan { root, revision } => {
                Ok(repoint::rescan(registered, root, revision))
            }
            LibraryWatchMessage::FilesystemChange(event) => {
                reseat(filesystem_change(registered, event), last_scan)
            }
            LibraryWatchMessage::DebounceElapsed => {
                reseat(debounce_elapsed(registered, last_scan), last_scan)
            }
        }
    }
}

fn filesystem_change(registered: Registered, event: Result<(), notify::Error>) -> Row {
    match (registered, event) {
        (registered @ Registered::Unrooted, Ok(()) | Err(_)) => {
            Err((registered, LibraryWatchRejection::Unwatched))
        }
        (Registered::On { root, .. }, Ok(())) => Ok((
            Registered::On {
                root,
                burst: Burst::Armed,
            },
            WatchIo::ArmDebounce,
        )),
        (Registered::On { root, burst }, Err(error)) => {
            Ok((Registered::On { root, burst }, WatchIo::Report(error)))
        }
    }
}

fn debounce_elapsed(registered: Registered, last_scan: Revision) -> Row {
    match registered {
        registered @ Registered::Unrooted => {
            Err((registered, LibraryWatchRejection::Unwatched))
        }
        Registered::On {
            root,
            burst: Burst::Armed,
        } => Ok((
            Registered::On {
                root: root.clone(),
                burst: Burst::Quiet,
            },
            WatchIo::Rescan {
                root,
                revision: last_scan,
            },
        )),
        registered @ Registered::On {
            burst: Burst::Quiet,
            ..
        } => Err((registered, LibraryWatchRejection::Settled)),
    }
}

fn reseat(row: Row, last_scan: Revision) -> Step {
    match row {
        Ok((registered, io)) => Ok((
            LibraryWatch {
                registered,
                last_scan,
            },
            io,
        )),
        Err((registered, reason)) => Err(Rejected {
            state: LibraryWatch {
                registered,
                last_scan,
            },
            reason,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{domain::Revision, update::Machine};
    use rstest::rstest;

    use crate::library::watch::{
        Burst,
        LibraryWatch,
        LibraryWatchMessage,
        LibraryWatchRejection,
        Registered,
        WatchIo,
    };

    fn root() -> PathBuf {
        PathBuf::from("/music")
    }

    fn other() -> PathBuf {
        PathBuf::from("/more-music")
    }

    fn revision(bumps: u64) -> Revision {
        (0..bumps).fold(Revision::default(), |revision, _| revision.next())
    }

    fn watch(registered: Registered) -> LibraryWatch {
        LibraryWatch {
            registered,
            last_scan: Revision::default(),
        }
    }

    fn unrooted() -> LibraryWatch {
        watch(Registered::Unrooted)
    }

    fn quiet() -> LibraryWatch {
        watch(Registered::On {
            root: root(),
            burst: Burst::Quiet,
        })
    }

    fn quiet_at(root: PathBuf) -> LibraryWatch {
        watch(Registered::On {
            root,
            burst: Burst::Quiet,
        })
    }

    fn armed() -> LibraryWatch {
        watch(Registered::On {
            root: root(),
            burst: Burst::Armed,
        })
    }

    fn scanned(state: LibraryWatch, bumps: u64) -> LibraryWatch {
        LibraryWatch {
            last_scan: revision(bumps),
            ..state
        }
    }

    fn rescan(root: PathBuf, bumps: u64) -> LibraryWatchMessage {
        LibraryWatchMessage::Rescan {
            root,
            revision: revision(bumps),
        }
    }

    fn change() -> LibraryWatchMessage {
        LibraryWatchMessage::FilesystemChange(Ok(()))
    }

    fn watcher_error() -> LibraryWatchMessage {
        LibraryWatchMessage::FilesystemChange(Err(notify::Error::generic(
            "stream stalled",
        )))
    }

    fn render(io: &WatchIo) -> String {
        match io {
            WatchIo::Move { from, to, revision } => {
                format!(
                    "move {} -> {} @ {}",
                    from.display(),
                    to.display(),
                    revision.get()
                )
            }
            WatchIo::ArmDebounce => "arm".to_string(),
            WatchIo::Rescan { root, revision } => {
                format!("rescan {} @ {}", root.display(), revision.get())
            }
            WatchIo::RegisterAndRescan { root, revision } => {
                format!(
                    "register and rescan {} @ {}",
                    root.display(),
                    revision.get()
                )
            }
            WatchIo::Report(error) => format!("report {error}"),
            WatchIo::Nothing => "nothing".to_string(),
        }
    }

    struct Cell {
        start: LibraryWatch,
        message: LibraryWatchMessage,
        next: LibraryWatch,
        io: &'static str,
    }

    #[rstest]
    #[case::unrooted_registers_on_the_first_scan(Cell {
        start: unrooted(),
        message: rescan(root(), 1),
        next: scanned(quiet(), 1),
        io: "register and rescan /music @ 1",
    })]
    #[case::on_rescans_its_root(Cell {
        start: quiet(),
        message: rescan(root(), 1),
        next: scanned(quiet(), 1),
        io: "rescan /music @ 1",
    })]
    #[case::armed_rescan_drops_the_burst(Cell {
        start: armed(),
        message: rescan(root(), 1),
        next: scanned(quiet(), 1),
        io: "rescan /music @ 1",
    })]
    #[case::on_moves_to_another_root(Cell {
        start: quiet(),
        message: rescan(other(), 1),
        next: scanned(quiet_at(other()), 1),
        io: "move /music -> /more-music @ 1",
    })]
    #[case::armed_move_drops_the_burst(Cell {
        start: armed(),
        message: rescan(other(), 1),
        next: scanned(quiet_at(other()), 1),
        io: "move /music -> /more-music @ 1",
    })]
    #[case::quiet_change_arms(Cell {
        start: quiet(),
        message: change(),
        next: armed(),
        io: "arm",
    })]
    #[case::armed_change_rearms(Cell {
        start: armed(),
        message: change(),
        next: armed(),
        io: "arm",
    })]
    #[case::quiet_watcher_error_is_reported(Cell {
        start: quiet(),
        message: watcher_error(),
        next: quiet(),
        io: "report stream stalled",
    })]
    #[case::armed_watcher_error_is_reported(Cell {
        start: armed(),
        message: watcher_error(),
        next: armed(),
        io: "report stream stalled",
    })]
    #[case::armed_deadline_rescans_at_the_last_seen_revision(Cell {
        start: scanned(armed(), 3),
        message: LibraryWatchMessage::DebounceElapsed,
        next: scanned(quiet(), 3),
        io: "rescan /music @ 3",
    })]
    fn a_cell_moves_the_watch_and_names_its_io(#[case] cell: Cell) {
        let (state, effect) = cell.start.transition(cell.message).unwrap();
        assert_eq!(state, cell.next);
        assert_eq!(render(&effect), cell.io);
    }

    #[rstest]
    #[case::unrooted_refuses_a_change(
        unrooted(),
        change(),
        LibraryWatchRejection::Unwatched
    )]
    #[case::unrooted_refuses_a_deadline(
        unrooted(),
        LibraryWatchMessage::DebounceElapsed,
        LibraryWatchRejection::Unwatched
    )]
    #[case::quiet_refuses_a_deadline(
        quiet(),
        LibraryWatchMessage::DebounceElapsed,
        LibraryWatchRejection::Settled
    )]
    fn a_refused_cell_hands_the_state_back(
        #[case] start: LibraryWatch,
        #[case] message: LibraryWatchMessage,
        #[case] reason: LibraryWatchRejection,
    ) {
        let expected = start.clone();
        let rejected = start.transition(message).err().unwrap();
        assert_eq!(rejected.state, expected);
        assert_eq!(rejected.reason, reason);
    }

    #[test]
    fn a_rescan_always_applies_even_if_the_revision_repeats() {
        let (state, _) = quiet().transition(rescan(root(), 1)).unwrap();
        let (state, effect) = state.transition(rescan(root(), 1)).unwrap();
        assert_eq!(state, scanned(quiet(), 1));
        assert_eq!(render(&effect), "rescan /music @ 1");
    }
}
