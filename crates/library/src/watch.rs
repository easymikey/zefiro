use std::path::PathBuf;

use kernel::{
    cmd::Cmd,
    domain::{io_error::IoError, revision::Revision},
    message::{LibraryError, LibraryEvent, LibrarySubject},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum LibraryWatch {
    #[default]
    Unrooted,
    Rooted {
        music_dir: PathBuf,
        revision: Revision,
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
    Rescan {
        music_dir: PathBuf,
        revision: Revision,
    },
    Changed(Result<(), IoError>),
    Elapsed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WatchEffect {
    Watch(PathBuf),
    Unwatch(PathBuf),
    Scan {
        music_dir: PathBuf,
        revision: Revision,
    },
    Arm,
}

impl Machine for LibraryWatch {
    type Message = LibraryWatchMessage;
    type Effect = Cmd<WatchEffect, LibraryEvent>;

    fn transition(
        &mut self,
        message: LibraryWatchMessage,
    ) -> Result<Cmd<WatchEffect, LibraryEvent>, Unhandled> {
        match message {
            LibraryWatchMessage::Rescan {
                music_dir,
                revision,
            } => Ok(self.rescan(music_dir, revision)),
            LibraryWatchMessage::Changed(Ok(())) => self.changed(),
            LibraryWatchMessage::Changed(Err(kind)) => self.failed(kind),
            LibraryWatchMessage::Elapsed => self.elapsed(),
        }
    }
}

impl LibraryWatch {
    fn changed(&mut self) -> Result<Cmd<WatchEffect, LibraryEvent>, Unhandled> {
        match self {
            LibraryWatch::Unrooted
            | LibraryWatch::Rooted {
                burst: Burst::Armed,
                ..
            } => Err(Unhandled),
            LibraryWatch::Rooted { burst, .. } => {
                *burst = Burst::Armed;
                Ok(Cmd::effect(WatchEffect::Arm))
            }
        }
    }

    fn failed(
        &self,
        source: IoError,
    ) -> Result<Cmd<WatchEffect, LibraryEvent>, Unhandled> {
        match self {
            LibraryWatch::Unrooted => Err(Unhandled),
            LibraryWatch::Rooted { music_dir, .. } => {
                Ok(Cmd::message(LibraryEvent::Error(LibraryError::Disk {
                    subject: LibrarySubject::Watch,
                    path: music_dir.clone(),
                    source,
                })))
            }
        }
    }

    fn elapsed(&mut self) -> Result<Cmd<WatchEffect, LibraryEvent>, Unhandled> {
        match self {
            LibraryWatch::Unrooted
            | LibraryWatch::Rooted {
                burst: Burst::Quiet,
                ..
            } => Err(Unhandled),
            LibraryWatch::Rooted {
                music_dir,
                revision,
                burst,
            } => {
                *burst = Burst::Quiet;
                Ok(Cmd::effect(WatchEffect::Scan {
                    music_dir: music_dir.clone(),
                    revision: *revision,
                }))
            }
        }
    }

    fn rescan(
        &mut self,
        target: PathBuf,
        revision: Revision,
    ) -> Cmd<WatchEffect, LibraryEvent> {
        let scan = WatchEffect::Scan {
            music_dir: target.clone(),
            revision,
        };
        let effects = match &*self {
            LibraryWatch::Unrooted => vec![WatchEffect::Watch(target.clone()), scan],
            LibraryWatch::Rooted { music_dir, .. } if *music_dir == target => {
                vec![scan]
            }
            LibraryWatch::Rooted { music_dir, .. } => vec![
                WatchEffect::Unwatch(music_dir.clone()),
                WatchEffect::Watch(target.clone()),
                scan,
            ],
        };
        *self = LibraryWatch::Rooted {
            music_dir: target,
            revision,
            burst: Burst::Quiet,
        };
        effects.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        cmd::Cmd,
        domain::{io_error::IoError, revision::Revision},
        message::{LibraryError, LibraryEvent, LibrarySubject},
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::watch::{Burst, LibraryWatch, LibraryWatchMessage, WatchEffect};

    fn music_dir() -> PathBuf {
        PathBuf::from("/music")
    }

    fn other() -> PathBuf {
        PathBuf::from("/more-music")
    }

    fn revision(bumps: u64) -> Revision {
        (0..bumps).fold(Revision::default(), |revision, _| revision.next())
    }

    fn rooted(music_dir: PathBuf, bumps: u64, burst: Burst) -> LibraryWatch {
        LibraryWatch::Rooted {
            music_dir,
            revision: revision(bumps),
            burst,
        }
    }

    fn quiet_at(music_dir: PathBuf, bumps: u64) -> LibraryWatch {
        rooted(music_dir, bumps, Burst::Quiet)
    }

    fn quiet(bumps: u64) -> LibraryWatch {
        quiet_at(music_dir(), bumps)
    }

    fn armed(bumps: u64) -> LibraryWatch {
        rooted(music_dir(), bumps, Burst::Armed)
    }

    fn rescan(music_dir: PathBuf, bumps: u64) -> LibraryWatchMessage {
        LibraryWatchMessage::Rescan {
            music_dir,
            revision: revision(bumps),
        }
    }

    fn describe_effect(effect: &WatchEffect) -> String {
        match effect {
            WatchEffect::Watch(dir) => format!("watch {}", dir.display()),
            WatchEffect::Unwatch(dir) => format!("unwatch {}", dir.display()),
            WatchEffect::Scan {
                music_dir,
                revision,
            } => format!("scan {} @ {}", music_dir.display(), revision.get()),
            WatchEffect::Arm => "arm".to_string(),
        }
    }

    fn describe(cmd: Cmd<WatchEffect, LibraryEvent>) -> String {
        let (effects, messages) = cmd.into_parts();
        assert!(messages.is_empty(), "the watch tells nothing: {messages:?}");
        let described: Vec<String> = effects.iter().map(describe_effect).collect();
        if described.is_empty() {
            "nothing".to_string()
        } else {
            described.join("; ")
        }
    }

    struct LibraryWatchRow {
        library_watch: LibraryWatch,
        message: LibraryWatchMessage,
        next_library_watch: LibraryWatch,
        effects: &'static str,
    }

    #[rstest]
    #[case::library_watch_unrooted_watches_and_scans_on_the_first_scan(LibraryWatchRow {
        library_watch: LibraryWatch::Unrooted,
        message: rescan(music_dir(), 1),
        next_library_watch: quiet(1),
        effects: "watch /music; scan /music @ 1",
    })]
    #[case::library_watch_rooted_rescans_its_root(LibraryWatchRow {
        library_watch: quiet(0),
        message: rescan(music_dir(), 1),
        next_library_watch: quiet(1),
        effects: "scan /music @ 1",
    })]
    #[case::armed_rescan_drops_the_burst(LibraryWatchRow {
        library_watch: armed(0),
        message: rescan(music_dir(), 1),
        next_library_watch: quiet(1),
        effects: "scan /music @ 1",
    })]
    #[case::rooted_moves_to_another_root(LibraryWatchRow {
        library_watch: quiet(0),
        message: rescan(other(), 1),
        next_library_watch: quiet_at(other(), 1),
        effects: "unwatch /music; watch /more-music; scan /more-music @ 1",
    })]
    #[case::armed_move_drops_the_burst(LibraryWatchRow {
        library_watch: armed(0),
        message: rescan(other(), 1),
        next_library_watch: quiet_at(other(), 1),
        effects: "unwatch /music; watch /more-music; scan /more-music @ 1",
    })]
    #[case::quiet_change_arms(LibraryWatchRow {
        library_watch: quiet(0),
        message: LibraryWatchMessage::Changed(Ok(())),
        next_library_watch: armed(0),
        effects: "arm",
    })]
    #[case::library_watch_rooted_elapse_rescans_at_its_revision(LibraryWatchRow {
        library_watch: armed(3),
        message: LibraryWatchMessage::Elapsed,
        next_library_watch: quiet(3),
        effects: "scan /music @ 3",
    })]
    fn a_library_watch_row_moves_the_watch_and_names_its_effects(
        #[case] row: LibraryWatchRow,
    ) {
        let mut library_watch = row.library_watch;
        let cmd = library_watch.transition(row.message).unwrap();
        assert_eq!(library_watch, row.next_library_watch);
        assert_eq!(describe(cmd), row.effects);
    }

    #[test]
    fn a_failure_under_a_root_reports_a_watch_error_for_that_root() {
        let mut library_watch = quiet(0);
        let cmd = library_watch
            .transition(LibraryWatchMessage::Changed(Err(IoError::Missing)))
            .unwrap();
        let (effects, messages) = cmd.into_parts();
        assert!(effects.is_empty());
        let [LibraryEvent::Error(error)] = messages.as_slice() else {
            panic!("expected one error message: {messages:?}");
        };
        assert_eq!(
            *error,
            LibraryError::Disk {
                subject: LibrarySubject::Watch,
                path: music_dir(),
                source: IoError::Missing,
            }
        );
    }

    #[rstest]
    #[case::unrooted_refuses_a_change(
        LibraryWatch::Unrooted,
        LibraryWatchMessage::Changed(Ok(()))
    )]
    #[case::unrooted_refuses_a_failure(
        LibraryWatch::Unrooted,
        LibraryWatchMessage::Changed(Err(IoError::Other))
    )]
    #[case::unrooted_refuses_an_elapse(
        LibraryWatch::Unrooted,
        LibraryWatchMessage::Elapsed
    )]
    #[case::armed_change_is_refused(armed(0), LibraryWatchMessage::Changed(Ok(())))]
    #[case::quiet_refuses_an_elapse(quiet(0), LibraryWatchMessage::Elapsed)]
    fn a_refused_row_hands_the_library_watch_back(
        #[case] library_watch: LibraryWatch,
        #[case] message: LibraryWatchMessage,
    ) {
        let expected = library_watch.clone();
        let mut handed = library_watch;
        let refused = handed.transition(message).err().unwrap();
        assert_eq!(handed, expected);
        assert_eq!(refused, Unhandled);
    }
}
