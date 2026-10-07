use kernel::{cmd::DiskCmd, update::machine::Driver};

use crate::{
    cover::CoverDecoded,
    dirs::LibraryDirs,
    driver::{LibraryDriver, LibraryEffect},
    error::Error,
    favorites,
    history,
    message::LibraryMessage,
    playlists,
    trash,
};

impl<P: FnMut(CoverDecoded)> Driver for LibraryDriver<P> {
    type Effect = LibraryEffect;

    fn execute(&mut self, effect: LibraryEffect) -> Option<LibraryMessage> {
        match effect {
            LibraryEffect::PublishCover(decoded) => {
                (self.publish_cover)(decoded);
                None
            }
            LibraryEffect::Execute(disk_cmd) => execute_disk(disk_cmd, &self.dirs)
                .unwrap_or_else(|error| Some(LibraryMessage::Error(error))),
        }
    }
}

fn execute_disk(
    disk_cmd: DiskCmd,
    dirs: &LibraryDirs,
) -> Result<Option<LibraryMessage>, Error> {
    match disk_cmd {
        DiskCmd::AppendHistory(entry) => history::append(dirs, &entry).map(|()| None),
        DiskCmd::SaveFavorites(favorites) => {
            favorites::save(dirs, &favorites).map(|()| None)
        }
        DiskCmd::LoadFavorites => favorites::load(dirs)
            .map(|favorites| Some(LibraryMessage::FavoritesLoaded(favorites))),
        DiskCmd::Trash(path) => {
            trash::move_to_trash(&path).map(|()| Some(LibraryMessage::Trashed(path)))
        }
        DiskCmd::LoadHistory(limit) => history::load(dirs, limit).map(
            |history::HistoryRead { entries, skipped }| {
                Some(LibraryMessage::HistoryLoaded { entries, skipped })
            },
        ),
        DiskCmd::SavePlaylist { name, tracks } => {
            playlists::save(dirs, &name, &tracks).map(|()| None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use kernel::{
        cmd::{CoverJob, DiskCmd, ScanMode},
        domain::{
            favorites::Favorites,
            geometry::Pixels,
            history::HistoryEntry,
            playlist::PlaylistFileName,
            revision::Revision,
            time::Moment,
            track::TrackSource,
        },
        message::LibraryEvent,
        update::machine::{Driver, LoopEffect, Machine},
    };
    use rstest::{fixture, rstest};
    use tempfile::TempDir;

    use crate::{
        cover::{CoverDecoded, CoverLookup},
        dirs::LibraryDirs,
        driver::{LibraryDriver, LibraryEffect},
        job::LibraryJob,
        message::LibraryMessage,
        test_support::{self, temp_dir_filters},
    };

    const AUDIO_EXTENSIONS: &[&str] = &["flac", "wav"];

    #[fixture]
    fn dirs() -> (TempDir, LibraryDirs) {
        let directory = tempfile::tempdir().unwrap();
        let library_dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        (directory, library_dirs)
    }

    fn unpublished(decoded: CoverDecoded) {
        let CoverDecoded { path, .. } = decoded;
        panic!("nothing is published here: {path:?}");
    }

    fn transitioned_event(
        message: Option<LibraryMessage>,
        library_driver: &mut LibraryDriver<fn(CoverDecoded)>,
    ) -> Option<LibraryEvent> {
        message.and_then(|message| {
            library_driver
                .transition(message)
                .unwrap()
                .into_parts()
                .1
                .into_iter()
                .next()
        })
    }

    fn execute(disk_cmd: DiskCmd, library_dirs: &LibraryDirs) -> Option<LibraryEvent> {
        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs.clone(), AUDIO_EXTENSIONS, unpublished);
        transitioned_event(
            library_driver.execute(LibraryEffect::Execute(disk_cmd)),
            &mut library_driver,
        )
    }

    fn scanned(
        music_dir: PathBuf,
        (revision, mode): (Revision, ScanMode),
        library_dirs: &LibraryDirs,
    ) -> Vec<LibraryEvent> {
        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs.clone(), AUDIO_EXTENSIONS, unpublished);
        let message = match mode {
            ScanMode::Fresh => LibraryJob::Scan {
                music_dir,
                revision,
                dirs: Arc::new(library_dirs.clone()),
                audio_extensions: AUDIO_EXTENSIONS,
            },
            ScanMode::Cached => LibraryJob::ReadCache {
                music_dir,
                revision,
                dirs: Arc::new(library_dirs.clone()),
            },
        }
        .run();
        let (effects, events) =
            library_driver.transition(message).unwrap().into_parts();
        let listed_events: Vec<LibraryEvent> = effects
            .into_iter()
            .flat_map(|effect| {
                let LoopEffect::Run(job) = effect else {
                    panic!("expected a job, got {effect:?}");
                };
                library_driver.transition(job.run()).unwrap().into_parts().1
            })
            .collect();
        events.into_iter().chain(listed_events).collect()
    }

    fn scan(
        music_dir: PathBuf,
        scanning: (Revision, ScanMode),
        library_dirs: &LibraryDirs,
    ) -> Option<LibraryEvent> {
        match <[LibraryEvent; 1]>::try_from(scanned(music_dir, scanning, library_dirs))
        {
            Ok([event]) => Some(event),
            Err(events) => panic!("expected one event, got {events:?}"),
        }
    }

    #[rstest]
    fn append_history_writes_an_entry_and_replies_with_nothing(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (_directory, library_dirs) = dirs;

        let event = execute(
            DiskCmd::AppendHistory(HistoryEntry::from_track(
                &test_support::titled("/music/song.flac", "Song"),
                Moment::default(),
            )),
            &library_dirs,
        );

        assert_eq!(event, None);
        assert!(library_dirs.data_dir.join("history.jsonl").is_file());
    }

    #[rstest]
    fn save_favorites_writes_the_file_and_replies_with_nothing(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (_directory, library_dirs) = dirs;
        let saved_favorites: Favorites = [TrackSource::Local("/music/a.flac".into())]
            .into_iter()
            .collect();

        let event = execute(DiskCmd::SaveFavorites(saved_favorites), &library_dirs);

        assert_eq!(event, None);
        assert!(library_dirs.data_dir.join("favorites.json").is_file());
    }

    #[rstest]
    fn load_favorites_replies_with_the_saved_set(dirs: (TempDir, LibraryDirs)) {
        let (_directory, library_dirs) = dirs;
        let saved_favorites: Favorites = [TrackSource::Local("/music/a.flac".into())]
            .into_iter()
            .collect();
        assert_eq!(
            execute(DiskCmd::SaveFavorites(saved_favorites), &library_dirs),
            None
        );

        let event = execute(DiskCmd::LoadFavorites, &library_dirs);

        insta::assert_debug_snapshot!(event);
    }

    #[rstest]
    fn trash_of_a_missing_file_answers_trashed(dirs: (TempDir, LibraryDirs)) {
        let (directory, library_dirs) = dirs;
        let missing = directory.path().join("never-existed.flac");

        let event = execute(DiskCmd::Trash(missing.clone()), &library_dirs);

        assert_eq!(event, Some(LibraryEvent::Trashed(missing)));
    }

    #[rstest]
    fn load_history_replies_with_the_appended_entry(dirs: (TempDir, LibraryDirs)) {
        let (_directory, library_dirs) = dirs;
        let appended = execute(
            DiskCmd::AppendHistory(HistoryEntry::from_track(
                &test_support::titled("/music/song.flac", "Song"),
                Moment::default(),
            )),
            &library_dirs,
        );
        assert_eq!(appended, None);

        let event = execute(DiskCmd::LoadHistory(10), &library_dirs);

        match event {
            Some(LibraryEvent::HistoryLoaded(entries)) => {
                let listed_sources: Vec<TrackSource> = entries
                    .iter()
                    .map(|entry: &HistoryEntry| entry.track_source.clone())
                    .collect();
                assert_eq!(
                    listed_sources,
                    vec![TrackSource::Local("/music/song.flac".into())]
                );
            }
            other => panic!("expected HistoryLoaded, got {other:?}"),
        }
    }

    #[rstest]
    fn save_playlist_writes_under_the_sanitised_name(dirs: (TempDir, LibraryDirs)) {
        let (_directory, library_dirs) = dirs;

        let event = execute(
            DiskCmd::SavePlaylist {
                name: PlaylistFileName::new("My Mix").unwrap(),
                tracks: vec![test_support::titled("/music/song.flac", "Song")],
            },
            &library_dirs,
        );

        assert_eq!(event, None);
        assert!(library_dirs.playlists_dir.join("My Mix.m3u8").is_file());
    }

    #[rstest]
    fn a_published_cover_reaches_the_sink(dirs: (TempDir, LibraryDirs)) {
        let (_directory, library_dirs) = dirs;
        let mut published = Vec::new();
        {
            let mut library_driver = LibraryDriver::new(
                library_dirs,
                AUDIO_EXTENSIONS,
                |decoded: CoverDecoded| {
                    published.push((decoded.path, decoded.side));
                },
            );
            let answer =
                library_driver.execute(LibraryEffect::PublishCover(CoverDecoded {
                    path: PathBuf::from("/music/one.flac"),
                    side: Pixels(64),
                    cover_lookup: CoverLookup::Missing,
                }));
            assert!(answer.is_none());
        }

        assert_eq!(
            published,
            vec![(PathBuf::from("/music/one.flac"), Pixels(64))]
        );
    }

    #[rstest]
    fn a_cover_job_runs_into_a_decoded_cover(dirs: (TempDir, LibraryDirs)) {
        let (directory, _library_dirs) = dirs;
        let path = directory.path().join("untagged.wav");
        std::fs::write(&path, include_bytes!("../tests/fixtures/tone.wav")).unwrap();

        let message = LibraryJob::DecodeCover {
            cover_job: CoverJob {
                path,
                side: Pixels(64),
            },
            revision: Revision::default(),
        }
        .run();

        assert!(matches!(
            message,
            LibraryMessage::CoverDecoded {
                decoded: Ok(CoverDecoded {
                    cover_lookup: CoverLookup::Missing,
                    ..
                }),
                ..
            }
        ));
    }

    #[test]
    fn jobs_order_covers_before_tags_before_scans() {
        let dirs = Arc::new(LibraryDirs {
            cache_dir: std::path::Path::new("/data").join("cache"),
            data_dir: std::path::Path::new("/data").join("data"),
            playlists_dir: std::path::Path::new("/data").join("playlists"),
        });
        let mut jobs = vec![
            LibraryJob::Scan {
                music_dir: PathBuf::from("/music"),
                revision: Revision::default(),
                dirs: Arc::clone(&dirs),
                audio_extensions: AUDIO_EXTENSIONS,
            },
            LibraryJob::Tag {
                music_dir: PathBuf::from("/music"),
                track_sources: Vec::new(),
                revision: Revision::default(),
                dirs,
            },
            LibraryJob::DecodeCover {
                cover_job: CoverJob {
                    path: PathBuf::from("/music/one.flac"),
                    side: Pixels(64),
                },
                revision: Revision::default(),
            },
        ];

        jobs.sort();

        assert!(matches!(
            jobs.as_slice(),
            [
                LibraryJob::DecodeCover { .. },
                LibraryJob::Tag { .. },
                LibraryJob::Scan { .. }
            ]
        ));
    }

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone.wav");

    fn scanned_music_dir(directory: &TempDir) -> PathBuf {
        let music_dir = directory.path().join("music");
        std::fs::create_dir_all(&music_dir).unwrap();
        std::fs::write(music_dir.join("a.wav"), TONE).unwrap();
        music_dir
    }

    #[rstest]
    fn a_fresh_scan_replies_with_the_tracks_it_found_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);

        let event = scan(
            music_dir,
            (Revision::default().next(), ScanMode::Fresh),
            &library_dirs,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn a_cached_scan_without_a_cache_lists_untagged_tracks_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);

        let event = scan(
            music_dir,
            (Revision::default().next().next(), ScanMode::Cached),
            &library_dirs,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn tag_tracks_answers_every_listed_path_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);
        let listed = vec![
            TrackSource::Local(music_dir.join("a.wav")),
            TrackSource::Local(music_dir.join("never-existed.flac")),
        ];

        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs.clone(), AUDIO_EXTENSIONS, unpublished);
        let event = transitioned_event(
            Some(
                LibraryJob::Tag {
                    music_dir,
                    track_sources: listed,
                    revision: Revision::default().next(),
                    dirs: Arc::new(library_dirs),
                }
                .run(),
            ),
            &mut library_driver,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn a_cached_scan_after_a_fresh_scan_answers_from_the_cache(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);
        let first = scan(
            music_dir.clone(),
            (Revision::default().next(), ScanMode::Fresh),
            &library_dirs,
        );
        assert!(matches!(first, Some(LibraryEvent::Loaded { .. })));

        let event = scan(
            music_dir,
            (Revision::default().next().next(), ScanMode::Cached),
            &library_dirs,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn a_corrupt_cache_is_passed_over_and_the_scan_lists_instead(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);
        std::fs::create_dir_all(&library_dirs.cache_dir).unwrap();
        std::fs::write(
            library_dirs.cache_dir.join("library.bin"),
            [6u8, 0xDE, 0xAD, 0xBE, 0xEF],
        )
        .unwrap();

        let events = scanned(
            music_dir,
            (Revision::default().next(), ScanMode::Cached),
            &library_dirs,
        );

        assert!(matches!(
            events.as_slice(),
            [LibraryEvent::Error(_), LibraryEvent::Listed { .. }]
        ));
    }

    #[rstest]
    fn a_scan_of_a_missing_folder_answers_an_error(dirs: (TempDir, LibraryDirs)) {
        let (directory, library_dirs) = dirs;

        let message = LibraryJob::Scan {
            music_dir: directory.path().join("absent"),
            revision: Revision::default(),
            dirs: Arc::new(library_dirs),
            audio_extensions: AUDIO_EXTENSIONS,
        }
        .run();

        assert!(matches!(message, LibraryMessage::Error(_)));
    }

    #[rstest]
    #[case::save_favorites(DiskCmd::SaveFavorites(Favorites::default()))]
    #[case::save_playlist(DiskCmd::SavePlaylist {
        name: PlaylistFileName::new("My Mix").unwrap(),
        tracks: Vec::new(),
    })]
    fn a_failed_disk_command_answers_an_error(#[case] disk_cmd: DiskCmd) {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("blocker");
        std::fs::write(&blocker, b"a file, not a directory").unwrap();
        let library_dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: blocker.join("data"),
            playlists_dir: blocker.join("playlists"),
        };
        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs, AUDIO_EXTENSIONS, unpublished);

        let message = library_driver.execute(LibraryEffect::Execute(disk_cmd));

        assert!(
            matches!(message, Some(LibraryMessage::Error(_))),
            "{message:?}"
        );
    }
}
