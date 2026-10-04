use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{LibraryCmd, LibraryEvent, Track, cmd::ScanMode};

use crate::{
    cache,
    dirs::LibraryDirs,
    error::Error,
    favorites,
    history,
    playlists,
    scan,
    trash,
};

pub fn execute(
    command: LibraryCmd,
    dirs: &LibraryDirs,
    decodable: &[&str],
) -> Result<Option<LibraryEvent>, Error> {
    match command {
        LibraryCmd::AppendHistory(entry) => {
            history::append(dirs, &entry).map(|()| None)
        }
        LibraryCmd::SaveFavorites(favorites) => {
            favorites::save(dirs, &favorites).map(|()| None)
        }
        LibraryCmd::LoadFavorites => favorites::load(dirs)
            .map(|loaded| Some(LibraryEvent::FavoritesLoaded(loaded))),
        LibraryCmd::Trash(path) => trash::move_to_trash(&path).map(|()| None),
        LibraryCmd::LoadHistory(limit) => history::load(dirs, limit)
            .map(|entries| Some(LibraryEvent::HistoryLoaded(entries))),
        LibraryCmd::Scan {
            music_dir,
            revision,
            mode: ScanMode::Full,
        } => {
            let paths = list_paths(&music_dir, decodable)?;
            let tracks = scan::read_tags(&paths);
            cache::save(dirs, &music_dir, &tracks)?;
            Ok(Some(LibraryEvent::Loaded { tracks, revision }))
        }
        LibraryCmd::Scan {
            music_dir,
            revision,
            mode: ScanMode::Cached,
        } => {
            let cached = cache::load(dirs, &music_dir);
            if !cached.is_empty() {
                return Ok(Some(LibraryEvent::Loaded {
                    tracks: cached,
                    revision,
                }));
            }
            let tracks = list_paths(&music_dir, decodable)?
                .iter()
                .map(|path| Arc::new(Track::listed(path)))
                .collect();
            Ok(Some(LibraryEvent::Listed { tracks, revision }))
        }
        LibraryCmd::SavePlaylist { name, tracks } => {
            playlists::save(dirs, &name, &tracks).map(|()| None)
        }
        LibraryCmd::PrefetchCover(_) => Ok(None),
        LibraryCmd::TagTracks {
            music_dir,
            paths,
            revision,
        } => {
            let tracks = scan::read_tags(&paths);
            cache::save(dirs, &music_dir, &tracks)?;
            Ok(Some(LibraryEvent::Tagged { tracks, revision }))
        }
    }
}

fn list_paths(music_dir: &Path, decodable: &[&str]) -> Result<Vec<PathBuf>, Error> {
    let scan::Listing { paths, first_error } = scan::list_dir(music_dir, decodable);
    match first_error {
        Some(error) if paths.is_empty() => Err(error),
        _ => Ok(paths),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        Favorites,
        HistoryEntry,
        LibraryCmd,
        LibraryEvent,
        TrackRef,
        cmd::ScanMode,
        domain::{Moment, Revision},
        playlist::PlaylistFileName,
    };
    use rstest::{fixture, rstest};
    use tempfile::TempDir;

    use crate::{
        dirs::LibraryDirs,
        execute::execute,
        test_support::{self, temp_dir_filters},
    };

    const DECODABLE: &[&str] = &["flac", "wav"];

    #[fixture]
    fn dirs() -> (TempDir, LibraryDirs) {
        let directory = tempfile::tempdir().unwrap();
        let paths = LibraryDirs::under(directory.path());
        (directory, paths)
    }

    #[rstest]
    fn append_history_writes_an_entry_and_replies_with_nothing(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (_directory, paths) = dirs;

        let event = execute(
            LibraryCmd::AppendHistory(HistoryEntry::from_track(
                &test_support::titled("/music/song.flac", "Song"),
                Moment::default(),
            )),
            &paths,
            DECODABLE,
        )
        .unwrap();

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

        let event =
            execute(LibraryCmd::SaveFavorites(saved), &paths, DECODABLE).unwrap();

        assert_eq!(event, None);
        assert!(paths.data_dir.join("favorites.json").is_file());
    }

    #[rstest]
    fn load_favorites_replies_with_the_saved_set(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;
        let saved: Favorites = [TrackRef::Local("/music/a.flac".into())]
            .into_iter()
            .collect();
        execute(LibraryCmd::SaveFavorites(saved), &paths, DECODABLE).unwrap();

        let event = execute(LibraryCmd::LoadFavorites, &paths, DECODABLE).unwrap();

        insta::assert_debug_snapshot!(event);
    }

    #[rstest]
    fn trash_of_a_missing_file_replies_with_nothing(dirs: (TempDir, LibraryDirs)) {
        let (directory, paths) = dirs;
        let missing = directory.path().join("never-existed.flac");

        let event = execute(LibraryCmd::Trash(missing), &paths, DECODABLE).unwrap();

        assert_eq!(event, None);
    }

    #[rstest]
    fn load_history_replies_with_the_appended_entry(dirs: (TempDir, LibraryDirs)) {
        let (_directory, paths) = dirs;
        execute(
            LibraryCmd::AppendHistory(HistoryEntry::from_track(
                &test_support::titled("/music/song.flac", "Song"),
                Moment::default(),
            )),
            &paths,
            DECODABLE,
        )
        .unwrap();

        let event = execute(LibraryCmd::LoadHistory(10), &paths, DECODABLE).unwrap();

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
            LibraryCmd::SavePlaylist {
                name: PlaylistFileName::new("My Mix").unwrap(),
                tracks: vec![test_support::titled("/music/song.flac", "Song")],
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

        assert_eq!(event, None);
        assert!(paths.playlists_dir.join("My Mix.m3u8").is_file());
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

        let event = execute(
            LibraryCmd::Scan {
                music_dir,
                revision: Revision::default().next(),
                mode: ScanMode::Full,
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

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

        let event = execute(
            LibraryCmd::Scan {
                music_dir,
                revision: Revision::default().next().next(),
                mode: ScanMode::Cached,
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

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
            music_dir.join("a.wav"),
            music_dir.join("never-existed.flac"),
        ];

        let event = execute(
            LibraryCmd::TagTracks {
                music_dir,
                paths: listed,
                revision: Revision::default().next(),
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

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
        execute(
            LibraryCmd::Scan {
                music_dir: music_dir.clone(),
                revision: Revision::default().next(),
                mode: ScanMode::Full,
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

        let event = execute(
            LibraryCmd::Scan {
                music_dir,
                revision: Revision::default().next().next(),
                mode: ScanMode::Cached,
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

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
            [5u8, 0xDE, 0xAD, 0xBE, 0xEF],
        )
        .unwrap();

        let event = execute(
            LibraryCmd::Scan {
                music_dir,
                revision: Revision::default().next(),
                mode: ScanMode::Cached,
            },
            &paths,
            DECODABLE,
        )
        .unwrap();

        assert!(matches!(event, Some(LibraryEvent::Listed { .. })));
    }
}
