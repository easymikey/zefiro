use std::{sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, CoverJob, DiskCmd, LibraryCmd, ScanMode},
    domain::{overlay::Subfolders, revision::Revision, track::Track},
    message::LibraryEvent,
    update::machine::{LoopCmd, LoopEffect, Machine, Unhandled, each_handled},
};

mod cover_requests;

use crate::{
    cover::{CoverCache, CoverDecoded, decoding::CoverDecoding},
    dirs::LibraryDirs,
    error::Error,
    job::LibraryJob,
    message::{LibraryMessage, LibraryTimer},
    watch::{LibraryWatch, LibraryWatchEffect, LibraryWatchMessage},
};

const DEBOUNCE: Duration = Duration::from_millis(500);

pub struct LibraryDriver<P> {
    pub(crate) dirs: Arc<LibraryDirs>,
    audio_extensions: &'static [&'static str],
    library_watch: LibraryWatch,
    decoding: CoverDecoding,
    cover_cache: CoverCache,
    wanted_cover_job: Option<CoverJob>,
    cover_revision: Revision,
    pub(crate) publish_cover: P,
}

impl<P> std::fmt::Debug for LibraryDriver<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryDriver")
            .field("dirs", &self.dirs)
            .field("library_watch", &self.library_watch)
            .field("decoding", &self.decoding)
            .finish_non_exhaustive()
    }
}

pub(crate) type LibraryLoopCmd =
    LoopCmd<LibraryEffect, LibraryJob, LibraryMessage, LibraryEvent>;

#[derive(Debug)]
pub enum LibraryEffect {
    PublishCover(CoverDecoded),
    Execute(DiskCmd),
}

impl<P> LibraryDriver<P> {
    #[must_use]
    pub fn new(
        dirs: LibraryDirs,
        audio_extensions: &'static [&'static str],
        publish_cover: P,
    ) -> Self {
        Self {
            dirs: Arc::new(dirs),
            audio_extensions,
            library_watch: LibraryWatch::default(),
            decoding: CoverDecoding::default(),
            cover_cache: CoverCache::default(),
            wanted_cover_job: None,
            cover_revision: Revision::default(),
            publish_cover,
        }
    }

    fn transition_cmd(
        &mut self,
        library_cmd: LibraryCmd,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        match library_cmd {
            LibraryCmd::Scan {
                music_dir,
                revision,
                mode,
            } => self.watched(
                LibraryWatchMessage::Rescan {
                    music_dir,
                    revision,
                },
                mode,
            ),
            LibraryCmd::DecodeCover(cover_job) => self.decode_cover(cover_job),
            LibraryCmd::PrefetchCover(cover_job) => self.prefetch(cover_job.path),
            LibraryCmd::TagTracks {
                music_dir,
                track_sources,
                revision,
            } => Ok(Cmd::effect(LoopEffect::Run(LibraryJob::Tag {
                music_dir,
                track_sources,
                revision,
                dirs: Arc::clone(&self.dirs),
            }))),
            LibraryCmd::Disk(disk) => Ok(Cmd::effect(LoopEffect::Execute(
                LibraryEffect::Execute(disk),
            ))),
            LibraryCmd::Probe { path, revision } => {
                Ok(Cmd::effect(LoopEffect::Run(LibraryJob::Probe {
                    path,
                    revision,
                })))
            }
            LibraryCmd::Subfolders { path, revision } => {
                Ok(Cmd::effect(LoopEffect::Run(LibraryJob::Subfolders {
                    path,
                    revision,
                    audio_extensions: self.audio_extensions,
                })))
            }
        }
    }

    fn watched(
        &mut self,
        message: LibraryWatchMessage,
        mode: ScanMode,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        let (effects, events) = self.library_watch.transition(message)?.into_parts();
        let library_loop_cmd: LibraryLoopCmd = effects
            .into_iter()
            .map(|effect| self.lift(effect, mode))
            .collect();
        Ok(drained(library_loop_cmd, events))
    }

    fn lift(
        &self,
        effect: LibraryWatchEffect,
        mode: ScanMode,
    ) -> LoopEffect<LibraryEffect, LibraryJob, LibraryMessage> {
        match effect {
            LibraryWatchEffect::Watch(path) => LoopEffect::Watch {
                path,
                changed: LibraryMessage::Changed,
            },
            LibraryWatchEffect::Unwatch(path) => LoopEffect::Unwatch(path),
            LibraryWatchEffect::StartDebounce => LoopEffect::After {
                delay: DEBOUNCE,
                message: LibraryMessage::Elapsed(LibraryTimer::Debounce),
            },
            LibraryWatchEffect::Scan {
                music_dir,
                revision,
            } => LoopEffect::Run(match mode {
                ScanMode::Fresh => LibraryJob::Scan {
                    music_dir,
                    revision,
                    dirs: Arc::clone(&self.dirs),
                    audio_extensions: self.audio_extensions,
                },
                ScanMode::Cached => LibraryJob::ReadCache {
                    music_dir,
                    revision,
                    dirs: Arc::clone(&self.dirs),
                },
            }),
        }
    }
}

fn drained(
    library_loop_cmd: LibraryLoopCmd,
    events: Vec<LibraryEvent>,
) -> LibraryLoopCmd {
    events
        .into_iter()
        .fold(library_loop_cmd, |cmd, event| cmd.then(Cmd::message(event)))
}

fn cached(
    library_job: LibraryJob,
    revision: Revision,
    tracks: Result<Vec<Arc<Track>>, Error>,
) -> LibraryLoopCmd {
    let listing = Cmd::effect(LoopEffect::Run(library_job));
    match tracks {
        Ok(tracks) if tracks.is_empty() => listing,
        Ok(tracks) => Cmd::message(LibraryEvent::Loaded { tracks, revision }),
        Err(error) => reported_error(&error).then(listing),
    }
}

fn listed(subfolders: Subfolders, revision: Revision) -> LibraryLoopCmd {
    Cmd::message(LibraryEvent::Subfolders {
        subfolders,
        revision,
    })
}

fn reported_error(error: &Error) -> LibraryLoopCmd {
    Cmd::message(LibraryEvent::Error(error.into()))
}

fn reported(event: LibraryEvent, skipped: Option<Error>) -> LibraryLoopCmd {
    skipped.into_iter().fold(Cmd::message(event), |cmd, error| {
        cmd.then(reported_error(&error))
    })
}

impl<P> Machine for LibraryDriver<P> {
    type Message = LibraryMessage;
    type Effect = LibraryLoopCmd;

    fn transition(
        &mut self,
        message: LibraryMessage,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        match message {
            LibraryMessage::Cmds(batch) => {
                each_handled(batch.cmds, |cmd| self.transition_cmd(cmd))
            }
            LibraryMessage::Changed(result) => {
                self.watched(LibraryWatchMessage::Changed(result), ScanMode::Fresh)
            }
            LibraryMessage::Elapsed(LibraryTimer::Debounce) => {
                self.watched(LibraryWatchMessage::Elapsed, ScanMode::Fresh)
            }
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
                    audio_extensions: self.audio_extensions,
                },
                revision,
                tracks,
            )),
            LibraryMessage::Scanned {
                tracks,
                revision,
                skipped,
            } => Ok(reported(LibraryEvent::Loaded { tracks, revision }, skipped)),
            LibraryMessage::Tagged {
                tracks,
                revision,
                skipped,
            } => Ok(reported(LibraryEvent::Tagged { tracks, revision }, skipped)),
            LibraryMessage::Listed {
                tracks,
                revision,
                skipped,
            } => Ok(reported(LibraryEvent::Listed { tracks, revision }, skipped)),
            LibraryMessage::FavoritesLoaded(favorites) => {
                Ok(Cmd::message(LibraryEvent::FavoritesLoaded(favorites)))
            }
            LibraryMessage::Trashed(path) => {
                Ok(Cmd::message(LibraryEvent::Trashed(path)))
            }
            LibraryMessage::Checked { verdict, revision } => {
                Ok(Cmd::message(LibraryEvent::Checked { verdict, revision }))
            }
            LibraryMessage::Subfolders {
                subfolders,
                revision,
            } => Ok(listed(subfolders, revision)),
            LibraryMessage::HistoryLoaded { entries, skipped } => {
                Ok(reported(LibraryEvent::HistoryLoaded(entries), skipped))
            }
            LibraryMessage::Error(error) => Ok(reported_error(&error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Instant,
    };

    use kernel::{
        cmd::{Cmds, DiskCmd, LibraryCmd, ScanMode},
        domain::{
            favorites::Favorites,
            geometry::Pixels,
            io_error::IoError,
            revision::Revision,
            track::Track,
        },
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        cover::{CoverDecoded, CoverError, CoverLookup},
        dirs::LibraryDirs,
        driver::{LibraryDriver, LibraryEffect, LibraryLoopCmd},
        error::Error,
        job::LibraryJob,
        message::{LibraryMessage, LibraryTimer},
    };

    fn unpublished(decoded: CoverDecoded) {
        let CoverDecoded { path, .. } = decoded;
        panic!("transition publishes nothing: {path:?}");
    }

    pub(crate) fn driver() -> LibraryDriver<fn(CoverDecoded)> {
        LibraryDriver::new(
            LibraryDirs {
                cache_dir: Path::new("/data").join("cache"),
                data_dir: Path::new("/data").join("data"),
                playlists_dir: Path::new("/data").join("playlists"),
            },
            &["flac"],
            unpublished,
        )
    }

    pub(crate) fn cmds(library_cmds: Vec<LibraryCmd>) -> LibraryMessage {
        LibraryMessage::from(Cmds {
            cmds: library_cmds,
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

    pub(crate) fn cover(path: &str) -> LibraryMessage {
        cover_sized(path, 64)
    }

    pub(crate) fn cover_sized(path: &str, side: u32) -> LibraryMessage {
        cmds(vec![LibraryCmd::DecodeCover(kernel::cmd::CoverJob {
            path: PathBuf::from(path),
            side: Pixels(side),
        })])
    }

    pub(crate) fn prefetch(path: &str) -> LibraryMessage {
        cmds(vec![LibraryCmd::PrefetchCover(kernel::cmd::CoverJob {
            path: PathBuf::from(path),
            side: Pixels(64),
        })])
    }

    pub(crate) fn decoded(path: &str, issued: usize) -> LibraryMessage {
        LibraryMessage::CoverDecoded {
            revision: (0..issued)
                .fold(Revision::default(), |revision, _| revision.next()),
            decoded: Ok(CoverDecoded {
                path: PathBuf::from(path),
                side: Pixels(64),
                cover_lookup: CoverLookup::Missing,
            }),
        }
    }

    pub(crate) fn failed(path: &str, issued: usize) -> LibraryMessage {
        LibraryMessage::CoverDecoded {
            revision: (0..issued)
                .fold(Revision::default(), |revision, _| revision.next()),
            decoded: Err(CoverError {
                path: PathBuf::from(path),
                source: image::ImageError::IoError(std::io::Error::other("no tag")),
            }),
        }
    }

    pub(crate) fn cover_state(driver: &LibraryDriver<fn(CoverDecoded)>) -> String {
        format!(
            "{:?} {:?} {:?} {:?} {:?}",
            driver.library_watch,
            driver.decoding,
            driver.cover_cache,
            driver.wanted_cover_job,
            driver.cover_revision
        )
    }

    fn describe_job(job: &LibraryJob) -> String {
        match job {
            LibraryJob::DecodeCover { cover_job, .. } => {
                format!("decode {} @ {}", cover_job.path.display(), cover_job.side.0)
            }
            LibraryJob::Tag {
                music_dir,
                revision,
                ..
            } => format!("tag {} @ {}", music_dir.display(), revision.get()),
            LibraryJob::Scan {
                music_dir,
                revision,
                ..
            } => format!("scan {} @ {}", music_dir.display(), revision.get()),
            LibraryJob::ReadCache {
                music_dir,
                revision,
                ..
            } => format!("read cache {} @ {}", music_dir.display(), revision.get()),
            LibraryJob::List {
                music_dir,
                revision,
                ..
            } => format!("list {} @ {}", music_dir.display(), revision.get()),
            LibraryJob::Probe { path, revision } => {
                format!("probe {} @ {}", path.display(), revision.get())
            }
            LibraryJob::Subfolders { path, revision, .. } => {
                format!("subfolders {} @ {}", path.display(), revision.get())
            }
        }
    }

    fn describe_effect(
        effect: &LoopEffect<LibraryEffect, LibraryJob, LibraryMessage>,
    ) -> String {
        match effect {
            LoopEffect::Run(job) => describe_job(job),
            LoopEffect::After { delay, message } => {
                format!("after {delay:?} {message:?}")
            }
            LoopEffect::Watch { path, .. } => format!("watch {}", path.display()),
            LoopEffect::Unwatch(path) => format!("unwatch {}", path.display()),
            LoopEffect::Execute(effect) => describe_executed(effect),
        }
    }

    fn describe_executed(effect: &LibraryEffect) -> String {
        match effect {
            LibraryEffect::PublishCover(decoded) => {
                let cover_lookup_text = match decoded.cover_lookup {
                    CoverLookup::Found(_) => "image",
                    CoverLookup::Missing => "missing",
                };
                format!(
                    "publish {} @ {} {cover_lookup_text}",
                    decoded.path.display(),
                    decoded.side.0
                )
            }
            LibraryEffect::Execute(disk) => format!("execute {}", describe_disk(disk)),
        }
    }

    fn describe_disk(disk_cmd: &DiskCmd) -> &'static str {
        match disk_cmd {
            DiskCmd::AppendHistory(_) => "append_history",
            DiskCmd::SaveFavorites(_) => "save_favorites",
            DiskCmd::LoadFavorites => "load_favorites",
            DiskCmd::Trash(_) => "trash",
            DiskCmd::LoadHistory(_) => "load_history",
            DiskCmd::SavePlaylist { .. } => "save_playlist",
        }
    }

    pub(crate) fn describe(library_loop_cmd: LibraryLoopCmd) -> String {
        let (effects, events) = library_loop_cmd.into_parts();
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

    pub(crate) struct LibraryRow {
        pub(crate) library_messages: Vec<LibraryMessage>,
        pub(crate) message: LibraryMessage,
        pub(crate) cmd: &'static str,
    }

    #[rstest]
    #[case::a_cached_scan_watches_and_scans_from_the_cache(LibraryRow {
        library_messages: Vec::new(),
        message: scan("/music", ScanMode::Cached),
        cmd: "watch /music; read cache /music @ 1",
    })]
    #[case::a_scan_of_another_folder_moves_the_watch(LibraryRow {
        library_messages: vec![scan("/music", ScanMode::Cached)],
        message: scan("/more", ScanMode::Fresh),
        cmd: "unwatch /music; watch /more; scan /more @ 1",
    })]
    #[case::a_change_arms_the_debounce(LibraryRow {
        library_messages: vec![scan("/music", ScanMode::Cached)],
        message: LibraryMessage::Changed(Ok(())),
        cmd: "after 500ms Elapsed(Debounce)",
    })]
    #[case::a_watch_failure_tells_an_error(LibraryRow {
        library_messages: vec![scan("/music", ScanMode::Cached)],
        message: LibraryMessage::Changed(Err(IoError::Missing)),
        cmd: "tell error",
    })]
    #[case::the_elapsed_debounce_rescans_fresh(LibraryRow {
        library_messages: vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed(Ok(()))],
        message: LibraryMessage::Elapsed(LibraryTimer::Debounce),
        cmd: "scan /music @ 1",
    })]
    #[case::tag_tracks_runs_a_tag_job(LibraryRow {
        library_messages: Vec::new(),
        message: cmds(vec![LibraryCmd::TagTracks {
            music_dir: PathBuf::from("/music"),
            track_sources: Vec::new(),
            revision: Revision::default().next(),
        }]),
        cmd: "tag /music @ 1",
    })]
    #[case::a_batch_keeps_its_command_order(LibraryRow {
        library_messages: Vec::new(),
        message: cmds(vec![
            LibraryCmd::Disk(DiskCmd::LoadFavorites),
            LibraryCmd::Scan {
                music_dir: PathBuf::from("/music"),
                revision: Revision::default().next(),
                mode: ScanMode::Fresh,
            },
            LibraryCmd::Disk(DiskCmd::LoadHistory(10)),
        ]),
        cmd: "execute load_favorites; watch /music; scan /music @ 1; execute load_history",
    })]
    #[case::a_scanned_answer_tells_loaded(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::Scanned {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: None,
        },
        cmd: "tell loaded",
    })]
    #[case::a_scanned_answer_with_a_skipped_file_tells_loaded_then_the_error(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::Scanned {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: Some(Error::NoUserDirs),
        },
        cmd: "tell loaded; tell error",
    })]
    #[case::a_tagged_answer_tells_tagged(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::Tagged {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: None,
        },
        cmd: "tell tagged",
    })]
    #[case::a_listed_answer_tells_listed(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::Listed {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: None,
        },
        cmd: "tell listed",
    })]
    #[case::a_favorites_answer_tells_favorites_loaded(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::FavoritesLoaded(Favorites::default()),
        cmd: "tell favorites_loaded",
    })]
    #[case::a_history_answer_tells_history_loaded_then_the_error(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::HistoryLoaded {
            entries: Vec::new(),
            skipped: Some(Error::NoUserDirs),
        },
        cmd: "tell history_loaded; tell error",
    })]
    #[case::an_error_tells_a_library_failure(LibraryRow {
        library_messages: Vec::new(),
        message: LibraryMessage::Error(Error::NoUserDirs),
        cmd: "tell error",
    })]
    fn a_row_steps_the_driver_and_names_its_cmd(#[case] row: LibraryRow) {
        let mut driver = driver();
        for message in row.library_messages {
            assert!(driver.transition(message).is_ok());
        }

        let cmd = driver.transition(row.message).unwrap();

        assert_eq!(describe(cmd), row.cmd);
    }

    #[rstest]
    #[case::a_cache_loads(Ok(vec![Arc::new(Track::listed(Path::new("/music/a.flac")))]), "tell loaded")]
    #[case::a_bad_cache_reports(Err(Error::NoUserDirs), "list /music @ 1; tell error")]
    fn a_cached_answer_loads_or_reports_then_lists(
        #[case] tracks: Result<Vec<Arc<Track>>, Error>,
        #[case] cmd: &str,
    ) {
        let message = LibraryMessage::Cached {
            music_dir: PathBuf::from("/music"),
            revision: Revision::default().next(),
            tracks,
        };

        let answer = driver().transition(message).unwrap();

        assert_eq!(describe(answer), cmd);
    }

    #[rstest]
    #[case::a_change_before_any_scan(Vec::new(), LibraryMessage::Changed(Ok(())))]
    #[case::a_debounce_without_a_change(
        vec![scan("/music", ScanMode::Fresh)],
        LibraryMessage::Elapsed(LibraryTimer::Debounce)
    )]
    #[case::a_second_change_while_armed_is_refused(
        vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed(Ok(()))],
        LibraryMessage::Changed(Ok(()))
    )]
    fn a_refused_row_is_unhandled(
        #[case] library_messages: Vec<LibraryMessage>,
        #[case] message: LibraryMessage,
    ) {
        let mut driver = driver();
        for step in library_messages {
            assert!(driver.transition(step).is_ok());
        }

        let before = cover_state(&driver);

        assert!(matches!(driver.transition(message), Err(Unhandled)));
        assert_eq!(cover_state(&driver), before);
    }
}
