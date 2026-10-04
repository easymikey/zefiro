use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, Cmds, CoverJob, LibraryCmd, ScanMode},
    domain::{
        favorites::Favorites,
        history::HistoryEntry,
        playlist::PlaylistFileName,
        revision::Revision,
        track::Track,
    },
    message::LibraryEvent,
    update::machine::{Machine, Unhandled},
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
    watch: LibraryWatch,
    decoding: CoverDecoding,
    covers: CoverCache,
    asked: Option<CoverJob>,
    cover_revision: Revision,
    pub(crate) publish: P,
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
pub enum LibraryEffect {
    Run(LibraryJob),
    After {
        delay: Duration,
        timer: LibraryTimer,
    },
    Watch(PathBuf),
    Unwatch(PathBuf),
    PublishCover(CoverDecoded),
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
            LibraryCmd::DecodeCover(job) => {
                return self.ask(job).unwrap_or_else(|Unhandled| Cmd::none());
            }
            LibraryCmd::PrefetchCover(job) => return self.prefetch(job.path),
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
        cmd::{Cmd, Cmds, LibraryCmd, ScanMode},
        domain::{geometry::Pixels, io_error::IoError, revision::Revision},
        message::LibraryEvent,
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        cover::{CoverArt, CoverDecoded, CoverError},
        dirs::LibraryDirs,
        driver::{DiskEffect, LibraryDriver, LibraryEffect},
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
            driver.watch,
            driver.decoding,
            driver.covers,
            driver.asked,
            driver.cover_revision
        )
    }

    fn loaded() -> LibraryEvent {
        LibraryEvent::Loaded {
            tracks: Vec::new(),
            revision: Revision::default(),
        }
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
        message: LibraryMessage::Changed(Ok(())),
        cmd: "after 500ms Debounce",
    })]
    #[case::a_second_change_waits_for_the_armed_debounce(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed(Ok(()))],
        message: LibraryMessage::Changed(Ok(())),
        cmd: "nothing",
    })]
    #[case::a_watch_failure_tells_an_error(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached)],
        message: LibraryMessage::Changed(Err(IoError::Missing)),
        cmd: "tell error",
    })]
    #[case::the_elapsed_debounce_rescans_in_full(LibraryRow {
        setup: vec![scan("/music", ScanMode::Cached), LibraryMessage::Changed(Ok(()))],
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
    #[case::a_prefetch_before_any_cover_does_nothing(LibraryRow {
        setup: Vec::new(),
        message: prefetch("/music/two.flac"),
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
    #[case::the_same_cover_while_it_decodes_does_nothing(LibraryRow {
        setup: vec![cover("/music/one.flac")],
        message: cover("/music/one.flac"),
        cmd: "nothing",
    })]
    #[case::a_duplicate_cover_after_its_decode_does_nothing(LibraryRow {
        setup: vec![cover("/music/one.flac"), decoded("/music/one.flac", 1)],
        message: cover("/music/one.flac"),
        cmd: "nothing",
    })]
    #[case::a_duplicate_cover_after_a_prefetch_does_nothing(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        message: cover("/music/one.flac"),
        cmd: "nothing",
    })]
    #[case::a_cover_at_a_new_side_while_it_decodes_restarts(LibraryRow {
        setup: vec![cover_sized("/music/one.flac", 64)],
        message: cover_sized("/music/one.flac", 96),
        cmd: "decode /music/one.flac @ 96",
    })]
    #[case::a_prefetch_while_a_cover_decodes_does_nothing(LibraryRow {
        setup: vec![cover("/music/one.flac")],
        message: prefetch("/music/two.flac"),
        cmd: "nothing",
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
    #[case::a_remembered_prefetch_is_not_published(LibraryRow {
        setup: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        message: prefetch("/music/two.flac"),
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

        let before = cover_state(&driver);

        assert!(matches!(driver.transition(message), Err(Unhandled)));
        assert_eq!(cover_state(&driver), before);
    }
}
