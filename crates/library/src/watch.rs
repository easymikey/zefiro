use std::path::PathBuf;

use kernel::{
    Cmd,
    domain::Revision,
    update::{Machine, Unhandled},
};

use crate::LibraryMessage;

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
pub(crate) enum LibraryWatchMessage {
    Rescan {
        music_dir: PathBuf,
        revision: Revision,
    },
    Changed,
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
    type Effect = Cmd<WatchEffect, LibraryMessage>;

    fn transition(
        &mut self,
        message: LibraryWatchMessage,
    ) -> Result<Cmd<WatchEffect, LibraryMessage>, Unhandled> {
        match message {
            LibraryWatchMessage::Rescan {
                music_dir,
                revision,
            } => Ok(self.rescan(music_dir, revision)),
            LibraryWatchMessage::Changed => self.changed(),
            LibraryWatchMessage::Elapsed => self.elapsed(),
        }
    }
}

impl LibraryWatch {
    fn changed(&mut self) -> Result<Cmd<WatchEffect, LibraryMessage>, Unhandled> {
        match &mut self.registered {
            Registered::Unrooted => Err(Unhandled),
            Registered::On {
                burst: Burst::Armed,
                ..
            } => Ok(Cmd::none()),
            Registered::On { burst, .. } => {
                *burst = Burst::Armed;
                Ok(Cmd::effect(WatchEffect::Arm))
            }
        }
    }

    fn elapsed(&mut self) -> Result<Cmd<WatchEffect, LibraryMessage>, Unhandled> {
        match &mut self.registered {
            Registered::Unrooted
            | Registered::On {
                burst: Burst::Quiet,
                ..
            } => Err(Unhandled),
            Registered::On { music_dir, burst } => {
                *burst = Burst::Quiet;
                Ok(Cmd::effect(WatchEffect::Scan {
                    music_dir: music_dir.clone(),
                    revision: self.last_scan,
                }))
            }
        }
    }

    fn rescan(
        &mut self,
        target: PathBuf,
        revision: Revision,
    ) -> Cmd<WatchEffect, LibraryMessage> {
        let scan = WatchEffect::Scan {
            music_dir: target.clone(),
            revision,
        };
        let effects = match &self.registered {
            Registered::Unrooted => vec![WatchEffect::Watch(target.clone()), scan],
            Registered::On { music_dir, .. } if *music_dir == target => vec![scan],
            Registered::On { music_dir, .. } => vec![
                WatchEffect::Unwatch(music_dir.clone()),
                WatchEffect::Watch(target.clone()),
                scan,
            ],
        };
        self.registered = Registered::On {
            music_dir: target,
            burst: Burst::Quiet,
        };
        self.last_scan = revision;
        effects.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        Cmd,
        domain::Revision,
        update::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        LibraryMessage,
        watch::{Burst, LibraryWatch, LibraryWatchMessage, Registered, WatchEffect},
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

    fn quiet_at(music_dir: PathBuf) -> LibraryWatch {
        watch(Registered::On {
            music_dir,
            burst: Burst::Quiet,
        })
    }

    fn quiet() -> LibraryWatch {
        quiet_at(music_dir())
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

    fn describe(cmd: Cmd<WatchEffect, LibraryMessage>) -> String {
        let (effects, messages) = cmd.into_parts();
        assert!(messages.is_empty(), "the watch tells nothing: {messages:?}");
        let described: Vec<String> = effects.iter().map(describe_effect).collect();
        if described.is_empty() {
            "nothing".to_string()
        } else {
            described.join("; ")
        }
    }

    struct WatchRow {
        start: LibraryWatch,
        message: LibraryWatchMessage,
        next: LibraryWatch,
        effects: &'static str,
    }

    #[rstest]
    #[case::unrooted_watches_and_scans_on_the_first_scan(WatchRow {
        start: unrooted(),
        message: rescan(music_dir(), 1),
        next: scanned(quiet(), 1),
        effects: "watch /music; scan /music @ 1",
    })]
    #[case::on_rescans_its_root(WatchRow {
        start: quiet(),
        message: rescan(music_dir(), 1),
        next: scanned(quiet(), 1),
        effects: "scan /music @ 1",
    })]
    #[case::armed_rescan_drops_the_burst(WatchRow {
        start: armed(),
        message: rescan(music_dir(), 1),
        next: scanned(quiet(), 1),
        effects: "scan /music @ 1",
    })]
    #[case::on_moves_to_another_root(WatchRow {
        start: quiet(),
        message: rescan(other(), 1),
        next: scanned(quiet_at(other()), 1),
        effects: "unwatch /music; watch /more-music; scan /more-music @ 1",
    })]
    #[case::armed_move_drops_the_burst(WatchRow {
        start: armed(),
        message: rescan(other(), 1),
        next: scanned(quiet_at(other()), 1),
        effects: "unwatch /music; watch /more-music; scan /more-music @ 1",
    })]
    #[case::quiet_change_arms(WatchRow {
        start: quiet(),
        message: LibraryWatchMessage::Changed,
        next: armed(),
        effects: "arm",
    })]
    #[case::armed_change_waits_for_the_armed_timer(WatchRow {
        start: armed(),
        message: LibraryWatchMessage::Changed,
        next: armed(),
        effects: "nothing",
    })]
    #[case::armed_elapse_rescans_at_the_last_seen_revision(WatchRow {
        start: scanned(armed(), 3),
        message: LibraryWatchMessage::Elapsed,
        next: scanned(quiet(), 3),
        effects: "scan /music @ 3",
    })]
    fn a_row_moves_the_watch_and_names_its_effects(#[case] row: WatchRow) {
        let mut state = row.start;
        let cmd = state.transition(row.message).unwrap();
        assert_eq!(state, row.next);
        assert_eq!(describe(cmd), row.effects);
    }

    #[rstest]
    #[case::unrooted_refuses_a_change(unrooted(), LibraryWatchMessage::Changed)]
    #[case::unrooted_refuses_an_elapse(unrooted(), LibraryWatchMessage::Elapsed)]
    #[case::quiet_refuses_an_elapse(quiet(), LibraryWatchMessage::Elapsed)]
    fn a_refused_row_hands_the_state_back(
        #[case] start: LibraryWatch,
        #[case] message: LibraryWatchMessage,
    ) {
        let expected = start.clone();
        let mut state = start;
        let refused = state.transition(message).err().unwrap();
        assert_eq!(state, expected);
        assert_eq!(refused, Unhandled);
    }
}
