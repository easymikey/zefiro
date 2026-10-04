#![forbid(unsafe_code)]

mod cache;
mod cover;
mod dirs;
mod error;
mod execute;
mod favorites;
pub mod files;
mod history;
mod playlists;
mod scan;
mod tags;
#[cfg(test)]
#[path = "../tests/unit/fixtures.rs"]
mod test_support;
mod trash;
mod watch;

use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    Cmd,
    Cmds,
    Favorites,
    HistoryEntry,
    LibraryCmd,
    LibraryEvent,
    Track,
    cmd::ScanMode,
    domain::Revision,
    playlist::PlaylistFileName,
    update::{Machine, Unhandled},
};

pub use crate::{
    cover::{CoverArt, CoverDecoded, CoverError, CoverJob},
    dirs::LibraryDirs,
    error::Error,
    execute::LibraryJob,
    playlists::load as load_playlist,
    tags::embedded_cover,
};
use crate::{
    cover::{CoverCache, CoverDecoding, CoverDecodingMessage},
    watch::{LibraryWatch, LibraryWatchMessage, WatchEffect},
};

const DEBOUNCE: Duration = Duration::from_millis(500);

pub struct LibraryDriver<P> {
    dirs: Arc<LibraryDirs>,
    decodable: &'static [&'static str],
    watch: LibraryWatch,
    decoding: CoverDecoding,
    covers: CoverCache,
    asked: Option<CoverJob>,
    cover_revision: Revision,
    publish: P,
}

impl<P> std::fmt::Debug for LibraryDriver<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryDriver")
            .field("dirs", &self.dirs)
            .field("watch", &self.watch)
            .field("decoding", &self.decoding)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum LibraryMessage {
    Cmds(Cmds<LibraryCmd>),
    Changed,
    Elapsed(LibraryTimer),
    Cover(CoverJob),
    CoverDecoded {
        revision: Revision,
        decoded: CoverDecoded,
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

#[derive(Debug)]
pub enum LibraryEffect {
    Run(LibraryJob),
    After {
        delay: Duration,
        timer: LibraryTimer,
    },
    Watch(PathBuf),
    Unwatch(PathBuf),
    Publish(CoverDecoded),
    Execute(DiskEffect),
}

#[derive(Debug)]
pub enum DiskEffect {
    AppendHistory(HistoryEntry),
    SaveFavorites(Favorites),
    LoadFavorites,
    Trash(PathBuf),
    LoadHistory(usize),
    SavePlaylist {
        name: PlaylistFileName,
        tracks: Vec<Arc<Track>>,
    },
}

impl<P> LibraryDriver<P> {
    #[must_use]
    pub fn new(
        dirs: LibraryDirs,
        decodable: &'static [&'static str],
        publish: P,
    ) -> Self {
        Self {
            dirs: Arc::new(dirs),
            decodable,
            watch: LibraryWatch::default(),
            decoding: CoverDecoding::default(),
            covers: CoverCache::default(),
            asked: None,
            cover_revision: Revision::default(),
            publish,
        }
    }

    fn command(&mut self, command: LibraryCmd) -> Cmd<LibraryEffect, LibraryEvent> {
        let disk = match command {
            LibraryCmd::Scan {
                music_dir,
                revision,
                mode,
            } => {
                return self
                    .watched(
                        LibraryWatchMessage::Rescan {
                            music_dir,
                            revision,
                        },
                        mode,
                    )
                    .unwrap_or_else(|Unhandled| Cmd::none());
            }
            LibraryCmd::PrefetchCover(path) => return self.prefetch(path),
            LibraryCmd::TagTracks {
                music_dir,
                tracks,
                revision,
            } => {
                return Cmd::effect(LibraryEffect::Run(LibraryJob::Tag {
                    music_dir,
                    tracks,
                    revision,
                    dirs: Arc::clone(&self.dirs),
                }));
            }
            LibraryCmd::AppendHistory(entry) => DiskEffect::AppendHistory(entry),
            LibraryCmd::SaveFavorites(favorites) => {
                DiskEffect::SaveFavorites(favorites)
            }
            LibraryCmd::LoadFavorites => DiskEffect::LoadFavorites,
            LibraryCmd::Trash(path) => DiskEffect::Trash(path),
            LibraryCmd::LoadHistory(limit) => DiskEffect::LoadHistory(limit),
            LibraryCmd::SavePlaylist { name, tracks } => {
                DiskEffect::SavePlaylist { name, tracks }
            }
        };
        Cmd::effect(LibraryEffect::Execute(disk))
    }

    fn watched(
        &mut self,
        message: LibraryWatchMessage,
        mode: ScanMode,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        let (effects, messages) = self.watch.transition(message)?.into_parts();
        let lifted: Cmd<LibraryEffect, LibraryEvent> = effects
            .into_iter()
            .map(|effect| self.lift(effect, mode))
            .collect();
        Ok(messages.into_iter().fold(lifted, |cmd, told| {
            cmd.then(
                self.transition(told)
                    .unwrap_or_else(|Unhandled| Cmd::none()),
            )
        }))
    }

    fn lift(&self, effect: WatchEffect, mode: ScanMode) -> LibraryEffect {
        match effect {
            WatchEffect::Watch(dir) => LibraryEffect::Watch(dir),
            WatchEffect::Unwatch(dir) => LibraryEffect::Unwatch(dir),
            WatchEffect::Arm => LibraryEffect::After {
                delay: DEBOUNCE,
                timer: LibraryTimer::Debounce,
            },
            WatchEffect::Scan {
                music_dir,
                revision,
            } => LibraryEffect::Run(LibraryJob::Scan {
                music_dir,
                revision,
                mode,
                dirs: Arc::clone(&self.dirs),
                decodable: self.decodable,
            }),
        }
    }

    fn ask(
        &mut self,
        job: CoverJob,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        if self.asked.as_ref() == Some(&job) {
            return Err(Unhandled);
        }
        let cmd = self.cover(job.clone())?;
        self.asked = Some(job);
        Ok(cmd)
    }

    fn prefetch(&mut self, path: PathBuf) -> Cmd<LibraryEffect, LibraryEvent> {
        let Some(side) = self.asked.as_ref().map(|asked| asked.side) else {
            return Cmd::none();
        };
        self.cover(CoverJob::new(path, side))
            .unwrap_or_else(|Unhandled| Cmd::none())
    }

    fn cover(
        &mut self,
        job: CoverJob,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        if let Some(decoded) = self.covers.answer(&job) {
            return Ok(Cmd::effect(LibraryEffect::Publish(decoded)));
        }
        let issued = job.issued(self.cover_revision.bump());
        let started = self
            .decoding
            .transition(CoverDecodingMessage::Request(issued))?;
        Ok(self.lift_decoding(started))
    }

    fn decoded(
        &mut self,
        revision: Revision,
        decoded: CoverDecoded,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        self.covers.remember(&decoded);
        let settled = self
            .decoding
            .transition(CoverDecodingMessage::Decoded(revision))?;
        Ok(self
            .lift_decoding(settled)
            .then(Cmd::effect(LibraryEffect::Publish(decoded))))
    }

    fn lift_decoding(
        &mut self,
        decoding: Cmd<CoverJob, LibraryMessage>,
    ) -> Cmd<LibraryEffect, LibraryEvent> {
        let (jobs, messages) = decoding.into_parts();
        let started: Cmd<LibraryEffect, LibraryEvent> = jobs
            .into_iter()
            .map(|job| LibraryEffect::Run(LibraryJob::Cover(job)))
            .collect();
        messages.into_iter().fold(started, |cmd, told| {
            cmd.then(
                self.transition(told)
                    .unwrap_or_else(|Unhandled| Cmd::none()),
            )
        })
    }
}

fn cached(
    list: LibraryJob,
    revision: Revision,
    tracks: Result<Vec<Arc<Track>>, Error>,
) -> Cmd<LibraryEffect, LibraryEvent> {
    let listing = Cmd::effect(LibraryEffect::Run(list));
    match tracks {
        Ok(tracks) if tracks.is_empty() => listing,
        Ok(tracks) => Cmd::message(LibraryEvent::Loaded { tracks, revision }),
        Err(error) => reported_error(&error).then(listing),
    }
}

fn reported_error(error: &Error) -> Cmd<LibraryEffect, LibraryEvent> {
    Cmd::message(LibraryEvent::Error(error.into()))
}

fn reported(
    event: LibraryEvent,
    skipped: Option<Error>,
) -> Cmd<LibraryEffect, LibraryEvent> {
    skipped.into_iter().fold(Cmd::message(event), |cmd, error| {
        cmd.then(reported_error(&error))
    })
}

impl<P> Machine for LibraryDriver<P> {
    type Message = LibraryMessage;
    type Effect = Cmd<LibraryEffect, LibraryEvent>;

    fn transition(
        &mut self,
        message: LibraryMessage,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        match message {
            LibraryMessage::Cmds(Cmds { cmds, .. }) => Ok(cmds
                .into_iter()
                .fold(Cmd::none(), |cmd, command| cmd.then(self.command(command)))),
            LibraryMessage::Changed => {
                self.watched(LibraryWatchMessage::Changed, ScanMode::Full)
            }
            LibraryMessage::Elapsed(LibraryTimer::Debounce) => {
                self.watched(LibraryWatchMessage::Elapsed, ScanMode::Full)
            }
            LibraryMessage::Cover(job) => self.ask(job),
            LibraryMessage::CoverDecoded { revision, decoded } => {
                self.decoded(revision, decoded)
            }
            LibraryMessage::Cached {
                music_dir,
                revision,
                tracks,
            } => Ok(cached(
                LibraryJob::List {
                    music_dir,
                    revision,
                    decodable: self.decodable,
                },
                revision,
                tracks,
            )),
            LibraryMessage::Scanned { event, skipped }
            | LibraryMessage::Tagged { event, skipped }
            | LibraryMessage::Executed { event, skipped } => {
                Ok(reported(event, skipped))
            }
            LibraryMessage::Error(error) => {
                Ok(Cmd::message(LibraryEvent::Error((&error).into())))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::Instant,
    };

    use kernel::{
        Cmd,
        Cmds,
        LibraryCmd,
        LibraryEvent,
        cmd::ScanMode,
        domain::{Revision, geometry::Pixels},
        update::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        CoverArt,
        CoverDecoded,
        CoverJob,
        DiskEffect,
        Error,
        LibraryDirs,
        LibraryDriver,
        LibraryEffect,
        LibraryJob,
        LibraryMessage,
        LibraryTimer,
    };

    fn unpublished(decoded: CoverDecoded) {
        let CoverDecoded { path, .. } = decoded;
        panic!("transition publishes nothing: {path:?}");
    }

    fn driver() -> LibraryDriver<fn(CoverDecoded)> {
        LibraryDriver::new(
            LibraryDirs::under(Path::new("/data")),
            &["flac"],
            unpublished,
        )
    }

    fn cmds(cmds: Vec<LibraryCmd>) -> LibraryMessage {
        LibraryMessage::from(Cmds {
            cmds,
            at: Instant::now(),
        })
    }

    fn scan(music_dir: &str, mode: ScanMode) -> LibraryMessage {
        cmds(vec![LibraryCmd::Scan {
            music_dir: PathBuf::from(music_dir),
            revision: Revision::default().next(),
            mode,
        }])
    }

    fn cover(path: &str) -> LibraryMessage {
        cover_sized(path, 64)
    }

    fn cover_sized(path: &str, side: u32) -> LibraryMessage {
        LibraryMessage::Cover(CoverJob::new(PathBuf::from(path), Pixels(side)))
    }

    fn prefetch(path: &str) -> LibraryMessage {
        cmds(vec![LibraryCmd::PrefetchCover(PathBuf::from(path))])
    }

    fn decoded(path: &str, issued: usize) -> LibraryMessage {
        LibraryMessage::CoverDecoded {
            revision: (0..issued)
                .fold(Revision::default(), |revision, _| revision.next()),
            decoded: CoverDecoded {
                path: PathBuf::from(path),
                side: Pixels(64),
                art: CoverArt::Missing,
            },
        }
    }

    fn loaded() -> LibraryEvent {
        LibraryEvent::Loaded {
            tracks: Vec::new(),
            revision: Revision::default(),
        }
    }

    fn describe_job(job: &LibraryJob) -> String {
        match job {
            LibraryJob::Cover(job) => {
                format!("decode {} @ {}", job.path.display(), job.side.0)
            }
            LibraryJob::Tag {
                music_dir,
                revision,
                ..
            } => format!("tag {} @ {}", music_dir.display(), revision.get()),
            LibraryJob::Scan {
                music_dir,
                revision,
                mode,
                ..
            } => format!("scan {mode:?} {} @ {}", music_dir.display(), revision.get()),
            LibraryJob::List {
                music_dir,
                revision,
                ..
            } => format!("list {} @ {}", music_dir.display(), revision.get()),
        }
    }

    fn describe_effect(effect: &LibraryEffect) -> String {
        match effect {
            LibraryEffect::Run(job) => describe_job(job),
            LibraryEffect::After { delay, timer } => {
                format!("after {delay:?} {timer:?}")
            }
            LibraryEffect::Watch(dir) => format!("watch {}", dir.display()),
            LibraryEffect::Unwatch(dir) => format!("unwatch {}", dir.display()),
            LibraryEffect::Publish(decoded) => {
                let art = match decoded.art {
                    CoverArt::Image(_) => "image",
                    CoverArt::Missing => "missing",
                    CoverArt::Error(_) => "error",
                };
                format!(
                    "publish {} @ {} {art}",
                    decoded.path.display(),
                    decoded.side.0
                )
            }
            LibraryEffect::Execute(disk) => format!("execute {}", describe_disk(disk)),
        }
    }

    fn describe_disk(disk: &DiskEffect) -> &'static str {
        match disk {
            DiskEffect::AppendHistory(_) => "append_history",
            DiskEffect::SaveFavorites(_) => "save_favorites",
            DiskEffect::LoadFavorites => "load_favorites",
            DiskEffect::Trash(_) => "trash",
            DiskEffect::LoadHistory(_) => "load_history",
            DiskEffect::SavePlaylist { .. } => "save_playlist",
        }
    }

    fn describe(cmd: Cmd<LibraryEffect, LibraryEvent>) -> String {
        let (effects, events) = cmd.into_parts();
        let described: Vec<String> = effects
            .iter()
            .map(describe_effect)
            .chain(
                events
                    .iter()
                    .map(|event| format!("tell {}", <&'static str>::from(event))),
            )
            .collect();
        if described.is_empty() {
            "nothing".to_string()
        } else {
            described.join("; ")
        }
    }

    struct LibraryRow {
        setup: Vec<LibraryMessage>,
        message: LibraryMessage,
        cmd: &'static str,
    }

    #[rstest]
    #[case::a_full_scan_watches_and_scans(LibraryRow {
        setup: Vec::new(),
        message: scan("/music", ScanMode::Full),
        cmd: "watch /music; scan Full /music @ 1",
    })]
    #[case::a_cached_scan_watches_and_scans_from_the_cache(LibraryRow {
        setup: Vec::new(),
        message: scan("/music", ScanMode::Cached),
        cmd: "watch /music; scan Cached /music @ 1",
    })]
    #[case::a_scan_of_another_folder_moves_the_watch(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached)],
        message: scan("/more", ScanMode::Full),
        cmd: "unwatch /music; watch /more; scan Full /more @ 1",
    })]
    #[case::a_change_arms_the_debounce(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached)],
        message: LibraryMessage::Changed,
        cmd: "after 500ms Debounce",
    })]
    #[case::a_second_change_waits_for_the_armed_debounce(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed],
        message: LibraryMessage::Changed,
        cmd: "nothing",
    })]
    #[case::the_elapsed_debounce_rescans_in_full(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed],
        message: LibraryMessage::Elapsed(LibraryTimer::Debounce),
        cmd: "scan Full /music @ 1",
    })]
    #[case::a_disk_command_executes(LibraryRow {
        setup: Vec::new(),
        message: cmds(vec![LibraryCmd::LoadFavorites]),
        cmd: "execute load_favorites",
    })]
    #[case::tag_tracks_runs_a_tag_job(LibraryRow {
        setup: Vec::new(),
        message: cmds(vec![LibraryCmd::TagTracks {
            music_dir: PathBuf::from("/music"),
            tracks: Vec::new(),
            revision: Revision::default().next(),
        }]),
        cmd: "tag /music @ 1",
    })]
    #[case::a_batch_keeps_its_command_order(LibraryRow {
        setup: Vec::new(),
        message: cmds(vec![
            LibraryCmd::LoadFavorites,
            LibraryCmd::Scan {
                music_dir: PathBuf::from("/music"),
                revision: Revision::default().next(),
                mode: ScanMode::Full,
            },
            LibraryCmd::LoadHistory(10),
        ]),
        cmd: "execute load_favorites; watch /music; scan Full /music @ 1; execute load_history",
    })]
    #[case::a_scanned_event_is_told(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Scanned {
            event: loaded(),
            skipped: None,
        },
        cmd: "tell loaded",
    })]
    #[case::a_tagged_event_is_told(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Tagged {
            event: loaded(),
            skipped: None,
        },
        cmd: "tell loaded",
    })]
    #[case::an_executed_event_is_told(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Executed {
            event: loaded(),
            skipped: None,
        },
        cmd: "tell loaded",
    })]
    #[case::an_error_tells_a_library_failure(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Error(Error::NoUserDirs),
        cmd: "tell error",
    })]
    #[case::a_cover_while_idle_decodes(LibraryRow {
        setup: Vec::new(),
        message: cover("/music/one.flac"),
        cmd: "decode /music/one.flac @ 64",
    })]
    #[case::a_decoded_cover_is_published(LibraryRow {
        setup: vec![cover("/music/one.flac")],
        message: decoded("/music/one.flac", 1),
        cmd: "publish /music/one.flac @ 64 missing",
    })]
    #[case::a_remembered_cover_is_published_without_a_decode(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            cover("/music/two.flac"),
        ],
        message: cover("/music/one.flac"),
        cmd: "publish /music/one.flac @ 64 missing",
    })]
    #[case::a_prefetch_before_any_cover_does_nothing(LibraryRow {
        setup: Vec::new(),
        message: prefetch("/music/two.flac"),
        cmd: "nothing",
    })]
    #[case::a_prefetch_uses_the_remembered_side(LibraryRow {
        setup: vec![cover_sized("/music/one.flac", 96)],
        message: prefetch("/music/two.flac"),
        cmd: "decode /music/two.flac @ 96",
    })]
    fn a_row_steps_the_driver_and_names_its_cmd(#[case] row: LibraryRow) {
        let mut driver = driver();
        for message in row.setup {
            assert!(driver.transition(message).is_ok());
        }

        let cmd = driver.transition(row.message).unwrap();

        assert_eq!(describe(cmd), row.cmd);
    }

    #[rstest]
    #[case::a_change_before_any_scan(Vec::new(), LibraryMessage::Changed)]
    #[case::a_debounce_without_a_change(
        vec![scan("/music", ScanMode::Full)],
        LibraryMessage::Elapsed(LibraryTimer::Debounce)
    )]
    #[case::the_same_cover_while_it_decodes(
        vec![cover("/music/one.flac")],
        cover("/music/one.flac")
    )]
    #[case::a_duplicate_cover_after_its_decode(
        vec![cover("/music/one.flac"), decoded("/music/one.flac", 1)],
        cover("/music/one.flac")
    )]
    #[case::a_duplicate_cover_after_a_prefetch(
        vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        cover("/music/one.flac")
    )]
    #[case::a_stale_decoded_cover(
        vec![cover("/music/one.flac"), cover("/music/two.flac")],
        decoded("/music/one.flac", 1)
    )]
    fn a_refused_row_is_unhandled(
        #[case] setup: Vec<LibraryMessage>,
        #[case] message: LibraryMessage,
    ) {
        let mut driver = driver();
        for step in setup {
            assert!(driver.transition(step).is_ok());
        }

        assert!(matches!(driver.transition(message), Err(Unhandled)));
    }
}
