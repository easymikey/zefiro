use kernel::{message::LibraryEvent, update::machine::Driver};

use crate::{
    cover::CoverDecoded,
    dirs::LibraryDirs,
    driver::{DiskEffect, LibraryDriver, LibraryEffect},
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
            LibraryEffect::Run(job) => Some(job.run()),
            LibraryEffect::After { .. }
            | LibraryEffect::Watch(_)
            | LibraryEffect::Unwatch(_) => None,
            LibraryEffect::PublishCover(decoded) => {
                (self.publish)(decoded);
                None
            }
            LibraryEffect::Execute(disk) => on_disk(disk, &self.dirs)
                .unwrap_or_else(|error| Some(LibraryMessage::Error(error))),
        }
    }
}

fn on_disk(
    disk: DiskEffect,
    dirs: &LibraryDirs,
) -> Result<Option<LibraryMessage>, Error> {
    match disk {
        DiskEffect::AppendHistory(entry) => {
            history::append(dirs, &entry).map(|()| None)
        }
        DiskEffect::SaveFavorites(favorites) => {
            favorites::save(dirs, &favorites).map(|()| None)
        }
        DiskEffect::LoadFavorites => favorites::load(dirs)
            .map(|loaded| executed(LibraryEvent::FavoritesLoaded(loaded), None)),
        DiskEffect::Trash(path) => trash::move_to_trash(&path).map(|()| None),
        DiskEffect::LoadHistory(limit) => history::load(dirs, limit).map(
            |history::HistoryRead { entries, skipped }| {
                executed(LibraryEvent::HistoryLoaded(entries), skipped)
            },
        ),
        DiskEffect::SavePlaylist { name, tracks } => {
            playlists::save(dirs, &name, &tracks).map(|()| None)
        }
    }
}

fn executed(event: LibraryEvent, skipped: Option<Error>) -> Option<LibraryMessage> {
    Some(LibraryMessage::Executed { event, skipped })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use kernel::{
        cmd::{CoverJob, ScanMode},
        domain::{
            favorites::Favorites,
            geometry::Pixels,
            history::HistoryEntry,
            playlist::PlaylistFileName,
            revision::Revision,
            time::Moment,
            track::TrackRef,
        },
        message::LibraryEvent,
        update::machine::{Driver, Machine},
    };
    use rstest::{fixture, rstest};
    use tempfile::TempDir;

    use crate::{
        cover::{CoverArt, CoverDecoded},
        dirs::LibraryDirs,
        driver::{DiskEffect, LibraryDriver, LibraryEffect},
        job::LibraryJob,
        message::{LibraryMessage, LibraryTimer},
        test_support::{self, temp_dir_filters},
    };

    const DECODABLE: &[&str] = &["flac", "wav"];

    #[fixture]
    fn dirs() -> (TempDir, LibraryDirs) {
        let directory = tempfile::tempdir().unwrap();
        let paths = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        (directory, paths)
    }

    fn unpublished(decoded: CoverDecoded) {
        let CoverDecoded { path, .. } = decoded;
        panic!("nothing is published here: {path:?}");
    }

    fn told(message: Option<LibraryMessage>) -> Option<LibraryEvent> {
        match message {
            None => None,
            Some(
                LibraryMessage::Executed { event, .. }
                | LibraryMessage::Scanned { event, .. }
                | LibraryMessage::Tagged { event, .. },
            ) => Some(event),
            Some(other) => panic!("expected an event, got {other:?}"),
        }
    }

    fn execute(disk: DiskEffect, paths: &LibraryDirs) -> Option<LibraryEvent> {
        let mut driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(paths.clone(), DECODABLE, unpublished);
        told(driver.execute(LibraryEffect::Execute(disk)))
    }

    fn scanned(
        music_dir: PathBuf,
        (revision, mode): (Revision, ScanMode),
        paths: &LibraryDirs,
    ) -> Vec<LibraryEvent> {
        let mut driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(paths.clone(), DECODABLE, unpublished);
        let message = LibraryJob::Scan {
            music_dir,
            revision,
            mode,
            dirs: Arc::new(paths.clone()),
            decodable: DECODABLE,
        }
        .run();
        let (effects, events) = driver.transition(message).unwrap().into_parts();
        let listed: Vec<LibraryEvent> = effects
            .into_iter()
            .flat_map(|effect| {
                let LibraryEffect::Run(job) = effect else {
                    panic!("expected a job, got {effect:?}");
                };
                driver.transition(job.run()).unwrap().into_parts().1
            })
            .collect();
        events.into_iter().chain(listed).collect()
    }

    fn scan(
        music_dir: PathBuf,
        scanning: (Revision, ScanMode),
        paths: &LibraryDirs,
    ) -> Option<LibraryEvent> {
        match <[LibraryEvent; 1]>::try_from(scanned(music_dir, scanning, paths)) {
            Ok([event]) => Some(event),
            Err(events) => panic!("expected one event, got {events:?}"),
        }
    }

    #[rstest]
    fn append_history_writes_an_entry_and_replies_with_nothing(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (_directory, paths) = dirs;

        let event = execute(
            DiskEffect::AppendHistory(HistoryEntry::from_track(
                &test_support::titled("/music/song.flac", "Song"),
                Moment::default(),
            )),
            &paths,
        );

        assert_eq!(event, None);
        assert!(paths.data_dir.join("history.jsonl").is_file());
    }

    #[rstest]
    fn save_favorites_writes_the_file_and_replies_with_nothing(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (_directory, paths) = dirs;
        let saved: Favorites = [TrackRef::Local("/music/a.flac".into())]
            .into_iter()
            .collect();

        let event = execute(DiskEffect::SaveFavorites(saved), &paths);

        assert_eq!(event, None);
        assert!(paths.data_dir.join("favorites.json").is_file());
    }

    #[rstest]
    fn load_favorites_replies_with_the_saved_set(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;
        let saved: Favorites = [TrackRef::Local("/music/a.flac".into())]
            .into_iter()
            .collect();
        assert_eq!(execute(DiskEffect::SaveFavorites(saved), &paths), None);

        let event = execute(DiskEffect::LoadFavorites, &paths);

        insta::assert_debug_snapshot!(event);
    }

    #[rstest]
    fn trash_of_a_missing_file_replies_with_nothing(dirs: (TempDir, LibraryDirs)) {
        let (directory, paths) = dirs;
        let missing = directory.path().join("never-existed.flac");

        let event = execute(DiskEffect::Trash(missing), &paths);

        assert_eq!(event, None);
    }

    #[rstest]
    fn load_history_replies_with_the_appended_entry(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;
        let appended = execute(
            DiskEffect::AppendHistory(HistoryEntry::from_track(
                &test_support::titled("/music/song.flac", "Song"),
                Moment::default(),
            )),
            &paths,
        );
        assert_eq!(appended, None);

        let event = execute(DiskEffect::LoadHistory(10), &paths);

        match event {
            Some(LibraryEvent::HistoryLoaded(entries)) => {
                let listed: Vec<TrackRef> = entries
                    .iter()
                    .map(|entry: &HistoryEntry| entry.track.clone())
                    .collect();
                assert_eq!(listed, vec![TrackRef::Local("/music/song.flac".into())]);
            }
            other => panic!("expected HistoryLoaded, got {other:?}"),
        }
    }

    #[rstest]
    fn save_playlist_writes_under_the_sanitised_name(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;

        let event = execute(
            DiskEffect::SavePlaylist {
                name: PlaylistFileName::new("My Mix").unwrap(),
                tracks: vec![test_support::titled("/music/song.flac", "Song")],
            },
            &paths,
        );

        assert_eq!(event, None);
        assert!(paths.playlists_dir.join("My Mix.m3u8").is_file());
    }

    #[rstest]
    fn a_stream_or_timer_effect_answers_nothing(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;
        let mut driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(paths, DECODABLE, unpublished);

        for effect in [
            LibraryEffect::Watch(PathBuf::from("/music")),
            LibraryEffect::Unwatch(PathBuf::from("/music")),
            LibraryEffect::After {
                delay: Duration::from_millis(500),
                timer: LibraryTimer::Debounce,
            },
        ] {
            assert!(driver.execute(effect).is_none());
        }
    }

    #[rstest]
    fn a_published_cover_reaches_the_sink(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;
        let mut published = Vec::new();
        {
            let mut driver =
                LibraryDriver::new(paths, DECODABLE, |decoded: CoverDecoded| {
                    published.push((decoded.path, decoded.side));
                });
            let answer = driver.execute(LibraryEffect::PublishCover(CoverDecoded {
                path: PathBuf::from("/music/one.flac"),
                side: Pixels(64),
                art: CoverArt::Missing,
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
        let (directory, _paths) = dirs;
        let path = directory.path().join("untagged.wav");
        std::fs::write(&path, include_bytes!("../tests/fixtures/tone.wav")).unwrap();

        let message = LibraryJob::Cover {
            job: CoverJob {
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
                    art: CoverArt::Missing,
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
                mode: ScanMode::Full,
                dirs: Arc::clone(&dirs),
                decodable: DECODABLE,
            },
            LibraryJob::Tag {
                music_dir: PathBuf::from("/music"),
                tracks: Vec::new(),
                revision: Revision::default(),
                dirs,
            },
            LibraryJob::Cover {
                job: CoverJob {
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
                LibraryJob::Cover { .. },
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
    fn a_full_scan_replies_with_the_tracks_it_found_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, paths) = dirs;
        let music_dir = scanned_music_dir(&directory);

        let event = scan(
            music_dir,
            (Revision::default().next(), ScanMode::Full),
            &paths,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn a_cached_scan_without_a_cache_lists_untagged_tracks_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, paths) = dirs;
        let music_dir = scanned_music_dir(&directory);

        let event = scan(
            music_dir,
            (Revision::default().next().next(), ScanMode::Cached),
            &paths,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn tag_tracks_answers_every_listed_path_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, paths) = dirs;
        let music_dir = scanned_music_dir(&directory);
        let listed = vec![
            TrackRef::Local(music_dir.join("a.wav")),
            TrackRef::Local(music_dir.join("never-existed.flac")),
        ];

        let event = told(Some(
            LibraryJob::Tag {
                music_dir,
                tracks: listed,
                revision: Revision::default().next(),
                dirs: Arc::new(paths),
            }
            .run(),
        ));

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn a_cached_scan_after_a_full_scan_answers_from_the_cache(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, paths) = dirs;
        let music_dir = scanned_music_dir(&directory);
        let first = scan(
            music_dir.clone(),
            (Revision::default().next(), ScanMode::Full),
            &paths,
        );
        assert!(matches!(first, Some(LibraryEvent::Loaded { .. })));

        let event = scan(
            music_dir,
            (Revision::default().next().next(), ScanMode::Cached),
            &paths,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn a_corrupt_cache_is_passed_over_and_the_scan_lists_instead(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, paths) = dirs;
        let music_dir = scanned_music_dir(&directory);
        std::fs::create_dir_all(&paths.cache_dir).unwrap();
        std::fs::write(
            paths.cache_dir.join("library.dir"),
            music_dir.to_string_lossy().as_bytes(),
        )
        .unwrap();
        std::fs::write(
            paths.cache_dir.join("library.bin"),
            [6u8, 0xDE, 0xAD, 0xBE, 0xEF],
        )
        .unwrap();

        let events = scanned(
            music_dir,
            (Revision::default().next(), ScanMode::Cached),
            &paths,
        );

        assert!(matches!(
            events.as_slice(),
            [LibraryEvent::Error(_), LibraryEvent::Listed { .. }]
        ));
    }

    #[rstest]
    fn a_scan_of_a_missing_folder_answers_an_error(dirs: (TempDir, LibraryDirs)) {
        let (directory, paths) = dirs;

        let message = LibraryJob::Scan {
            music_dir: directory.path().join("absent"),
            revision: Revision::default(),
            mode: ScanMode::Full,
            dirs: Arc::new(paths),
            decodable: DECODABLE,
        }
        .run();

        assert!(matches!(message, LibraryMessage::Error(_)));
    }
}
