use std::{sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, Cmds, CoverJob, DiskCmd, LibraryCmd, ScanMode},
    domain::{revision::Revision, track::Track},
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
    watch::{LibraryWatch, LibraryWatchMessage, WatchEffect},
};

const DEBOUNCE: Duration = Duration::from_millis(500);

pub struct LibraryDriver<P> {
    pub(crate) dirs: Arc<LibraryDirs>,
    decodable: &'static [&'static str],
    library_watch: LibraryWatch,
    decoding: CoverDecoding,
    covers: CoverCache,
    asked: Option<CoverJob>,
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
        decodable: &'static [&'static str],
        publish_cover: P,
    ) -> Self {
        Self {
            dirs: Arc::new(dirs),
            decodable,
            library_watch: LibraryWatch::default(),
            decoding: CoverDecoding::default(),
            covers: CoverCache::default(),
            asked: None,
            cover_revision: Revision::default(),
            publish_cover,
        }
    }

    fn command(&mut self, command: LibraryCmd) -> Result<LibraryLoopCmd, Unhandled> {
        match command {
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
            LibraryCmd::DecodeCover(job) => self.ask(job),
            LibraryCmd::PrefetchCover(job) => self.prefetch(job.path),
            LibraryCmd::TagTracks {
                music_dir,
                tracks,
                revision,
            } => Ok(Cmd::effect(LoopEffect::Run(LibraryJob::Tag {
                music_dir,
                tracks,
                revision,
                dirs: Arc::clone(&self.dirs),
            }))),
            LibraryCmd::Disk(disk) => Ok(Cmd::effect(LoopEffect::Execute(
                LibraryEffect::Execute(disk),
            ))),
        }
    }

    fn watched(
        &mut self,
        message: LibraryWatchMessage,
        mode: ScanMode,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        let (effects, events) = self.library_watch.transition(message)?.into_parts();
        let lifted: LibraryLoopCmd = effects
            .into_iter()
            .map(|effect| self.lift(effect, mode))
            .collect();
        Ok(drained(lifted, events))
    }

    fn lift(
        &self,
        effect: WatchEffect,
        mode: ScanMode,
    ) -> LoopEffect<LibraryEffect, LibraryJob, LibraryMessage> {
        match effect {
            WatchEffect::Watch(path) => LoopEffect::Watch {
                path,
                item: LibraryMessage::Changed,
            },
            WatchEffect::Unwatch(path) => LoopEffect::Unwatch(path),
            WatchEffect::Arm => LoopEffect::After {
                delay: DEBOUNCE,
                message: LibraryMessage::Elapsed(LibraryTimer::Debounce),
            },
            WatchEffect::Scan {
                music_dir,
                revision,
            } => LoopEffect::Run(match mode {
                ScanMode::Full => LibraryJob::Scan {
                    music_dir,
                    revision,
                    dirs: Arc::clone(&self.dirs),
                    decodable: self.decodable,
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
    list: LibraryJob,
    revision: Revision,
    tracks: Result<Vec<Arc<Track>>, Error>,
) -> LibraryLoopCmd {
    let listing = Cmd::effect(LoopEffect::Run(list));
    match tracks {
        Ok(tracks) if tracks.is_empty() => listing,
        Ok(tracks) => Cmd::message(LibraryEvent::Loaded { tracks, revision }),
        Err(error) => reported_error(&error).then(listing),
    }
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
            LibraryMessage::Cmds(Cmds { cmds, .. }) => {
                each_handled(cmds, |command| self.command(command))
            }
            LibraryMessage::Changed(result) => {
                self.watched(LibraryWatchMessage::Changed(result), ScanMode::Full)
            }
            LibraryMessage::Elapsed(LibraryTimer::Debounce) => {
                self.watched(LibraryWatchMessage::Elapsed, ScanMode::Full)
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
                    decodable: self.decodable,
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
            LibraryMessage::HistoryLoaded { entries, skipped } => {
                Ok(reported(LibraryEvent::HistoryLoaded(entries), skipped))
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
        cmd::{Cmds, DiskCmd, LibraryCmd, ScanMode},
        domain::{
            favorites::Favorites,
            geometry::Pixels,
            io_error::IoError,
            revision::Revision,
        },
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        cover::{CoverArt, CoverDecoded, CoverError},
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

    fn driver() -> LibraryDriver<fn(CoverDecoded)> {
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
        cmds(vec![LibraryCmd::DecodeCover(kernel::cmd::CoverJob {
            path: PathBuf::from(path),
            side: Pixels(side),
        })])
    }

    fn prefetch(path: &str) -> LibraryMessage {
        cmds(vec![LibraryCmd::PrefetchCover(kernel::cmd::CoverJob {
            path: PathBuf::from(path),
            side: Pixels(64),
        })])
    }

    fn decoded(path: &str, issued: usize) -> LibraryMessage {
        LibraryMessage::CoverDecoded {
            revision: (0..issued)
                .fold(Revision::default(), |revision, _| revision.next()),
            decoded: Ok(CoverDecoded {
                path: PathBuf::from(path),
                side: Pixels(64),
                art: CoverArt::Missing,
            }),
        }
    }

    fn failed(path: &str, issued: usize) -> LibraryMessage {
        LibraryMessage::CoverDecoded {
            revision: (0..issued)
                .fold(Revision::default(), |revision, _| revision.next()),
            decoded: Err(CoverError {
                path: PathBuf::from(path),
                source: image::ImageError::IoError(std::io::Error::other("no tag")),
            }),
        }
    }

    fn cover_state(driver: &LibraryDriver<fn(CoverDecoded)>) -> String {
        format!(
            "{:?} {:?} {:?} {:?} {:?}",
            driver.library_watch,
            driver.decoding,
            driver.covers,
            driver.asked,
            driver.cover_revision
        )
    }

    fn describe_job(job: &LibraryJob) -> String {
        match job {
            LibraryJob::Cover { job, .. } => {
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
                let art = match decoded.art {
                    CoverArt::Image(_) => "image",
                    CoverArt::Missing => "missing",
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

    fn describe(library_loop_cmd: LibraryLoopCmd) -> String {
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

    struct LibraryRow {
        setup: Vec<LibraryMessage>,
        message: LibraryMessage,
        cmd: &'static str,
    }

    #[rstest]
    #[case::a_full_scan_watches_and_scans(LibraryRow {
        setup: Vec::new(),
        message: scan("/music", ScanMode::Full),
        cmd: "watch /music; scan /music @ 1",
    })]
    #[case::a_cached_scan_watches_and_scans_from_the_cache(LibraryRow {
        setup: Vec::new(),
        message: scan("/music", ScanMode::Cached),
        cmd: "watch /music; read cache /music @ 1",
    })]
    #[case::a_scan_of_another_folder_moves_the_watch(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached)],
        message: scan("/more", ScanMode::Full),
        cmd: "unwatch /music; watch /more; scan /more @ 1",
    })]
    #[case::a_change_arms_the_debounce(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached)],
        message: LibraryMessage::Changed(Ok(())),
        cmd: "after 500ms Elapsed(Debounce)",
    })]
    #[case::a_watch_failure_tells_an_error(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached)],
        message: LibraryMessage::Changed(Err(IoError::Missing)),
        cmd: "tell error",
    })]
    #[case::the_elapsed_debounce_rescans_in_full(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed(Ok(()))],
        message: LibraryMessage::Elapsed(LibraryTimer::Debounce),
        cmd: "scan /music @ 1",
    })]
    #[case::a_disk_command_executes(LibraryRow {
        setup: Vec::new(),
        message: cmds(vec![LibraryCmd::Disk(DiskCmd::LoadFavorites)]),
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
            LibraryCmd::Disk(DiskCmd::LoadFavorites),
            LibraryCmd::Scan {
                music_dir: PathBuf::from("/music"),
                revision: Revision::default().next(),
                mode: ScanMode::Full,
            },
            LibraryCmd::Disk(DiskCmd::LoadHistory(10)),
        ]),
        cmd: "execute load_favorites; watch /music; scan /music @ 1; execute load_history",
    })]
    #[case::a_scanned_answer_tells_loaded(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Scanned {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: None,
        },
        cmd: "tell loaded",
    })]
    #[case::a_scanned_answer_with_a_skipped_file_tells_loaded_then_the_error(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Scanned {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: Some(Error::NoUserDirs),
        },
        cmd: "tell loaded; tell error",
    })]
    #[case::a_tagged_answer_tells_tagged(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Tagged {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: None,
        },
        cmd: "tell tagged",
    })]
    #[case::a_listed_answer_tells_listed(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::Listed {
            tracks: Vec::new(),
            revision: Revision::default(),
            skipped: None,
        },
        cmd: "tell listed",
    })]
    #[case::a_favorites_answer_tells_favorites_loaded(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::FavoritesLoaded(Favorites::default()),
        cmd: "tell favorites_loaded",
    })]
    #[case::a_history_answer_tells_history_loaded_then_the_error(LibraryRow {
        setup: Vec::new(),
        message: LibraryMessage::HistoryLoaded {
            entries: Vec::new(),
            skipped: Some(Error::NoUserDirs),
        },
        cmd: "tell history_loaded; tell error",
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
    #[case::a_failed_cover_is_published_missing_and_told(LibraryRow {
        setup: vec![cover("/music/one.flac")],
        message: failed("/music/one.flac", 1),
        cmd: "publish /music/one.flac @ 64 missing; tell error",
    })]
    #[case::a_failed_cover_is_decoded_again_when_asked_again(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            failed("/music/one.flac", 1),
            cover("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        message: cover("/music/one.flac"),
        cmd: "decode /music/one.flac @ 64",
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
    #[case::a_batch_keeps_its_handled_commands_when_a_prefetch_is_refused(LibraryRow {
        setup: Vec::new(),
        message: cmds(vec![
            LibraryCmd::PrefetchCover(kernel::cmd::CoverJob {
                path: PathBuf::from("/music/two.flac"),
                side: Pixels(64),
            }),
            LibraryCmd::Disk(DiskCmd::LoadFavorites),
        ]),
        cmd: "execute load_favorites",
    })]
    #[case::a_cover_asked_while_its_prefetch_decodes_waits_for_it(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
        ],
        message: cover("/music/two.flac"),
        cmd: "nothing",
    })]
    #[case::a_prefetch_uses_the_remembered_side(LibraryRow {
        setup: vec![
            cover_sized("/music/one.flac", 96),
            LibraryMessage::CoverDecoded {
                revision: Revision::default().next(),
                decoded: Ok(CoverDecoded {
                    path: PathBuf::from("/music/one.flac"),
                    side: Pixels(96),
                    art: CoverArt::Missing,
                }),
            },
        ],
        message: prefetch("/music/two.flac"),
        cmd: "decode /music/two.flac @ 96",
    })]
    #[case::a_cover_at_a_new_side_while_it_decodes_restarts(LibraryRow {
        setup: vec![cover_sized("/music/one.flac", 64)],
        message: cover_sized("/music/one.flac", 96),
        cmd: "decode /music/one.flac @ 96",
    })]
    #[case::a_decoded_prefetch_is_remembered_not_published(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
        ],
        message: decoded("/music/two.flac", 2),
        cmd: "nothing",
    })]
    #[case::a_cover_asked_while_its_prefetch_decodes_is_published(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            cover("/music/two.flac"),
        ],
        message: decoded("/music/two.flac", 2),
        cmd: "publish /music/two.flac @ 64 missing",
    })]
    #[case::a_prefetched_cover_is_published_from_memory(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        message: cover("/music/two.flac"),
        cmd: "publish /music/two.flac @ 64 missing",
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
    #[case::a_change_before_any_scan(Vec::new(), LibraryMessage::Changed(Ok(())))]
    #[case::a_debounce_without_a_change(
        vec![scan("/music", ScanMode::Full)],
        LibraryMessage::Elapsed(LibraryTimer::Debounce)
    )]
    #[case::a_second_change_while_armed_is_refused(
        vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed(Ok(()))],
        LibraryMessage::Changed(Ok(()))
    )]
    #[case::a_prefetch_before_any_cover_is_refused(
        Vec::new(),
        prefetch("/music/two.flac")
    )]
    #[case::a_prefetch_while_a_cover_decodes_is_refused(
        vec![cover("/music/one.flac")],
        prefetch("/music/two.flac")
    )]
    #[case::a_prefetch_of_a_remembered_cover_is_refused(
        vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        prefetch("/music/two.flac")
    )]
    #[case::a_prefetch_of_the_oldest_remembered_cover_is_refused(
        vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        prefetch("/music/one.flac")
    )]
    #[case::a_stale_decoded_cover(
        vec![cover("/music/one.flac"), cover("/music/two.flac")],
        decoded("/music/one.flac", 1)
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
    #[case::a_batch_whose_commands_are_all_rejected(
        vec![cover("/music/one.flac")],
        cmds(vec![
            LibraryCmd::DecodeCover(kernel::cmd::CoverJob {
                path: PathBuf::from("/music/one.flac"),
                side: Pixels(64),
            }),
            LibraryCmd::DecodeCover(kernel::cmd::CoverJob {
                path: PathBuf::from("/music/one.flac"),
                side: Pixels(64),
            }),
        ])
    )]
    fn a_refused_row_is_unhandled(
        #[case] setup: Vec<LibraryMessage>,
        #[case] message: LibraryMessage,
    ) {
        let mut driver = driver();
        for step in setup {
            assert!(driver.transition(step).is_ok());
        }

        let before = cover_state(&driver);

        assert!(matches!(driver.transition(message), Err(Unhandled)));
        assert_eq!(cover_state(&driver), before);
    }
}
