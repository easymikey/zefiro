use std::path::PathBuf;

use kernel::{domain::Revision, update::Machine};

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

#[derive(Debug)]
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
}

impl Machine for LibraryWatch {
    type Message = WatchMessage;
    type Error = WatchError;
    type Effect = WatchEffect;

    fn transition(&mut self, message: WatchMessage) -> Result<WatchEffect, WatchError> {
        match message {
            WatchMessage::Rescan {
                music_dir,
                revision,
            } => Ok(self.rescan(music_dir, revision)),
            WatchMessage::FilesystemChange(event) => self.filesystem_change(event),
            WatchMessage::DebounceElapsed => self.debounce_elapsed(),
        }
    }
}

impl LibraryWatch {
    fn filesystem_change(
        &mut self,
        event: Result<(), notify::Error>,
    ) -> Result<WatchEffect, WatchError> {
        match (&mut self.registered, event) {
            (Registered::Unrooted, Ok(()) | Err(_)) => Err(WatchError::Unwatched),
            (Registered::On { burst, .. }, Ok(())) => {
                *burst = Burst::Armed;
                Ok(WatchEffect::ArmDebounce)
            }
            (Registered::On { .. }, Err(error)) => Ok(WatchEffect::Report(error)),
        }
    }

    fn debounce_elapsed(&mut self) -> Result<WatchEffect, WatchError> {
        match &mut self.registered {
            Registered::Unrooted => Err(WatchError::Unwatched),
            Registered::On {
                burst: Burst::Quiet,
                ..
            } => Err(WatchError::Settled),
            Registered::On { music_dir, burst } => {
                *burst = Burst::Quiet;
                Ok(WatchEffect::Rescan {
                    music_dir: music_dir.clone(),
                    revision: self.last_scan,
                })
            }
        }
    }

    fn rescan(&mut self, target: PathBuf, revision: Revision) -> WatchEffect {
        let effect = match &self.registered {
            Registered::Unrooted => WatchEffect::RegisterAndRescan {
                music_dir: target.clone(),
                revision,
            },
            Registered::On { music_dir, .. } if *music_dir == target => {
                WatchEffect::Rescan {
                    music_dir: target.clone(),
                    revision,
                }
            }
            Registered::On { music_dir, .. } => WatchEffect::Rename {
                from: music_dir.clone(),
                to: target.clone(),
                revision,
            },
        };
        self.registered = Registered::On {
            music_dir: target,
            burst: Burst::Quiet,
        };
        self.last_scan = revision;
        effect
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
        let mut state = cell.start;
        let effect = state.transition(cell.message).unwrap();
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
        let mut state = start;
        let refused = state.transition(message).err().unwrap();
        assert_eq!(state, expected);
        assert_eq!(refused, reason);
    }

    #[test]
    fn a_rescan_always_applies_even_if_the_revision_repeats() {
        let mut state = quiet();
        state.transition(rescan(music_dir(), 1)).unwrap();
        let effect = state.transition(rescan(music_dir(), 1)).unwrap();
        assert_eq!(state, scanned(quiet(), 1));
        assert_eq!(render(&effect), "rescan /music @ 1");
    }
}
