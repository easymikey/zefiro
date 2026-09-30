use std::path::PathBuf;

use kernel::{
    domain::Revision,
    update::{Machine, Rejected},
};

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
        music_dir: PathBuf,
        burst: Burst,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Burst {
    Quiet,
    Armed,
}

#[derive(Debug)]
pub(crate) enum WatchMessage {
    Rescan {
        music_dir: PathBuf,
        revision: Revision,
    },
    FilesystemChange(Result<(), notify::Error>),
    DebounceElapsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchError {
    Unwatched,
    Settled,
}

#[derive(Debug, Default)]
pub(crate) enum WatchEffect {
    Rename {
        from: PathBuf,
        to: PathBuf,
        revision: Revision,
    },
    ArmDebounce,
    Rescan {
        music_dir: PathBuf,
        revision: Revision,
    },
    RegisterAndRescan {
        music_dir: PathBuf,
        revision: Revision,
    },
    Report(notify::Error),
    #[default]
    Nothing,
}

type Step = Result<(LibraryWatch, WatchEffect), Rejected<LibraryWatch>>;
pub(crate) type Row = Result<(Registered, WatchEffect), (Registered, WatchError)>;

impl Machine for LibraryWatch {
    type Message = WatchMessage;
    type Error = WatchError;
    type Effect = WatchEffect;

    fn transition(self, message: WatchMessage) -> Step {
        let LibraryWatch {
            registered,
            last_scan,
        } = self;
        match message {
            WatchMessage::Rescan {
                music_dir,
                revision,
            } => Ok(rescan(registered, music_dir, revision)),
            WatchMessage::FilesystemChange(event) => {
                reseat(filesystem_change(registered, event), last_scan)
            }
            WatchMessage::DebounceElapsed => {
                reseat(debounce_elapsed(registered, last_scan), last_scan)
            }
        }
    }
}

fn filesystem_change(registered: Registered, event: Result<(), notify::Error>) -> Row {
    match (registered, event) {
        (registered @ Registered::Unrooted, Ok(()) | Err(_)) => {
            Err((registered, WatchError::Unwatched))
        }
        (Registered::On { music_dir, .. }, Ok(())) => Ok((
            Registered::On {
                music_dir,
                burst: Burst::Armed,
            },
            WatchEffect::ArmDebounce,
        )),
        (Registered::On { music_dir, burst }, Err(error)) => Ok((
            Registered::On { music_dir, burst },
            WatchEffect::Report(error),
        )),
    }
}

fn debounce_elapsed(registered: Registered, last_scan: Revision) -> Row {
    match registered {
        registered @ Registered::Unrooted => Err((registered, WatchError::Unwatched)),
        Registered::On {
            music_dir,
            burst: Burst::Armed,
        } => Ok((
            Registered::On {
                music_dir: music_dir.clone(),
                burst: Burst::Quiet,
            },
            WatchEffect::Rescan {
                music_dir,
                revision: last_scan,
            },
        )),
        registered @ Registered::On {
            burst: Burst::Quiet,
            ..
        } => Err((registered, WatchError::Settled)),
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

fn rescan(
    registered: Registered,
    music_dir: PathBuf,
    revision: Revision,
) -> (LibraryWatch, WatchEffect) {
    let (registered, io) = rewatch(registered, music_dir, revision);
    (
        LibraryWatch {
            registered,
            last_scan: revision,
        },
        io,
    )
}

fn rewatch(
    registered: Registered,
    target: PathBuf,
    revision: Revision,
) -> (Registered, WatchEffect) {
    match registered {
        Registered::Unrooted => (
            Registered::On {
                music_dir: target.clone(),
                burst: Burst::Quiet,
            },
            WatchEffect::RegisterAndRescan {
                music_dir: target,
                revision,
            },
        ),
        Registered::On { music_dir, .. } if music_dir == target => (
            Registered::On {
                music_dir,
                burst: Burst::Quiet,
            },
            WatchEffect::Rescan {
                music_dir: target,
                revision,
            },
        ),
        Registered::On { music_dir, .. } => (
            Registered::On {
                music_dir: target.clone(),
                burst: Burst::Quiet,
            },
            WatchEffect::Rename {
                from: music_dir,
                to: target,
                revision,
            },
        ),
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
        Registered,
        WatchEffect,
        WatchError,
        WatchMessage,
    };

    fn music_dir() -> PathBuf {
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
            music_dir: music_dir(),
            burst: Burst::Quiet,
        })
    }

    fn quiet_at(music_dir: PathBuf) -> LibraryWatch {
        watch(Registered::On {
            music_dir,
            burst: Burst::Quiet,
        })
    }

    fn armed() -> LibraryWatch {
        watch(Registered::On {
            music_dir: music_dir(),
            burst: Burst::Armed,
        })
    }

    fn scanned(state: LibraryWatch, bumps: u64) -> LibraryWatch {
        LibraryWatch {
            last_scan: revision(bumps),
            ..state
        }
    }

    fn rescan(music_dir: PathBuf, bumps: u64) -> WatchMessage {
        WatchMessage::Rescan {
            music_dir,
            revision: revision(bumps),
        }
    }

    fn change() -> WatchMessage {
        WatchMessage::FilesystemChange(Ok(()))
    }

    fn watcher_error() -> WatchMessage {
        WatchMessage::FilesystemChange(Err(notify::Error::generic("stream stalled")))
    }

    fn render(io: &WatchEffect) -> String {
        match io {
            WatchEffect::Rename { from, to, revision } => {
                format!(
                    "move {} -> {} @ {}",
                    from.display(),
                    to.display(),
                    revision.get()
                )
            }
            WatchEffect::ArmDebounce => "arm".to_string(),
            WatchEffect::Rescan {
                music_dir,
                revision,
            } => {
                format!("rescan {} @ {}", music_dir.display(), revision.get())
            }
            WatchEffect::RegisterAndRescan {
                music_dir,
                revision,
            } => {
                format!(
                    "register and rescan {} @ {}",
                    music_dir.display(),
                    revision.get()
                )
            }
            WatchEffect::Report(error) => format!("report {error}"),
            WatchEffect::Nothing => "nothing".to_string(),
        }
    }

    struct Cell {
        start: LibraryWatch,
        message: WatchMessage,
        next: LibraryWatch,
        io: &'static str,
    }

    #[rstest]
    #[case::unrooted_registers_on_the_first_scan(Cell {
        start: unrooted(),
        message: rescan(music_dir(), 1),
        next: scanned(quiet(), 1),
        io: "register and rescan /music @ 1",
    })]
    #[case::on_rescans_its_root(Cell {
        start: quiet(),
        message: rescan(music_dir(), 1),
        next: scanned(quiet(), 1),
        io: "rescan /music @ 1",
    })]
    #[case::armed_rescan_drops_the_burst(Cell {
        start: armed(),
        message: rescan(music_dir(), 1),
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
        message: WatchMessage::DebounceElapsed,
        next: scanned(quiet(), 3),
        io: "rescan /music @ 3",
    })]
    fn a_cell_moves_the_watch_and_names_its_io(#[case] cell: Cell) {
        let (state, effect) = cell.start.transition(cell.message).unwrap();
        assert_eq!(state, cell.next);
        assert_eq!(render(&effect), cell.io);
    }

    #[rstest]
    #[case::unrooted_refuses_a_change(unrooted(), change(), WatchError::Unwatched)]
    #[case::unrooted_refuses_a_deadline(
        unrooted(),
        WatchMessage::DebounceElapsed,
        WatchError::Unwatched
    )]
    #[case::quiet_refuses_a_deadline(
        quiet(),
        WatchMessage::DebounceElapsed,
        WatchError::Settled
    )]
    fn a_refused_cell_hands_the_state_back(
        #[case] start: LibraryWatch,
        #[case] message: WatchMessage,
        #[case] reason: WatchError,
    ) {
        let expected = start.clone();
        let rejected = start.transition(message).err().unwrap();
        assert_eq!(rejected.state, expected);
        assert_eq!(rejected.reason, reason);
    }

    #[test]
    fn a_rescan_always_applies_even_if_the_revision_repeats() {
        let (state, _) = quiet().transition(rescan(music_dir(), 1)).unwrap();
        let (state, effect) = state.transition(rescan(music_dir(), 1)).unwrap();
        assert_eq!(state, scanned(quiet(), 1));
        assert_eq!(render(&effect), "rescan /music @ 1");
    }
}
