use std::{collections::HashSet, path::PathBuf, sync::Arc};

use kernel::{
    HistoryEntry,
    LibraryCmd,
    LibraryEvent,
    Tags,
    Track,
    domain::{Revision, ScanMode, UnixSeconds},
    playlist::PlaylistFileName,
};
use library::{
    CacheMiss,
    LibraryDirs,
    LibraryWarning,
    execute,
    test_support::{self, tmp_filters},
};
use rstest::{fixture, rstest};
use tempfile::TempDir;

const DECODABLE: &[&str] = &["flac", "wav"];

fn track(path: &str) -> Arc<Track> {
    Arc::new(test_support::track(
        path,
        Tags {
            title: Some("Song".to_string()),
            ..Tags::default()
        },
    ))
}

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

    let executed = execute(
        LibraryCmd::AppendHistory {
            track: track("/music/song.flac"),
            at: UnixSeconds::UNSTAMPED,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(executed.event, None);
    assert!(paths.data_dir.join("history.jsonl").is_file());
}

#[rstest]
fn save_favorites_writes_the_file_and_replies_with_nothing(
    dirs: (TempDir, LibraryDirs),
) {
    let (_directory, paths) = dirs;
    let mut saved = HashSet::new();
    saved.insert(PathBuf::from("/music/a.flac"));

    let executed = execute(
        LibraryCmd::SaveFavorites(Arc::new(saved)),
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(executed.event, None);
    assert!(paths.data_dir.join("favorites.json").is_file());
}

#[rstest]
fn load_favorites_replies_with_the_saved_set(dirs: (TempDir, LibraryDirs)) {
    let (_directory, paths) = dirs;
    let mut saved = HashSet::new();
    saved.insert(PathBuf::from("/music/a.flac"));
    execute(
        LibraryCmd::SaveFavorites(Arc::new(saved)),
        &paths,
        DECODABLE,
    )
    .unwrap();

    let executed = execute(LibraryCmd::LoadFavorites, &paths, DECODABLE).unwrap();

    insta::assert_debug_snapshot!(executed.event);
}

#[rstest]
fn trash_of_a_missing_file_replies_with_nothing(dirs: (TempDir, LibraryDirs)) {
    let (directory, paths) = dirs;
    let missing = directory.path().join("never-existed.flac");

    let executed = execute(LibraryCmd::Trash(missing), &paths, DECODABLE).unwrap();

    assert_eq!(executed.event, None);
}

#[rstest]
fn load_history_replies_with_the_appended_entry(dirs: (TempDir, LibraryDirs)) {
    let (_directory, paths) = dirs;
    execute(
        LibraryCmd::AppendHistory {
            track: track("/music/song.flac"),
            at: UnixSeconds::UNSTAMPED,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    let executed =
        execute(LibraryCmd::LoadHistory { limit: 10 }, &paths, DECODABLE).unwrap();

    match executed.event {
        Some(LibraryEvent::HistoryLoaded(entries)) => {
            let listed: Vec<PathBuf> = entries
                .iter()
                .map(|entry: &HistoryEntry| entry.path.clone())
                .collect();
            assert_eq!(listed, vec![PathBuf::from("/music/song.flac")]);
        }
        other => panic!("expected HistoryLoaded, got {other:?}"),
    }
    assert!(executed.warnings.is_empty());
}

#[rstest]
fn save_playlist_writes_under_the_sanitised_name(dirs: (TempDir, LibraryDirs)) {
    let (_directory, paths) = dirs;

    let executed = execute(
        LibraryCmd::SavePlaylist {
            name: PlaylistFileName::new("My Mix").unwrap(),
            tracks: vec![track("/music/song.flac")],
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(executed.event, None);
    assert!(paths.playlists_dir.join("My Mix.m3u8").is_file());
}

const TONE: &[u8] = include_bytes!("../fixtures/tone.wav");

fn scanned_music_dir(directory: &TempDir) -> PathBuf {
    let music_dir = directory.path().join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("a.wav"), TONE).unwrap();
    music_dir
}

#[rstest]
fn rescan_replies_with_the_tracks_it_found_under_the_revision_it_was_given(
    dirs: (TempDir, LibraryDirs),
) {
    let (directory, paths) = dirs;
    let music_dir = scanned_music_dir(&directory);

    let executed = execute(
        LibraryCmd::Scan {
            music_dir,
            revision: Revision::UNSTAMPED.next(),
            mode: ScanMode::Full,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.event);
    });
}

#[rstest]
fn scan_library_lists_placeholder_tracks_under_the_revision_it_was_given(
    dirs: (TempDir, LibraryDirs),
) {
    let (directory, paths) = dirs;
    let music_dir = scanned_music_dir(&directory);

    let executed = execute(
        LibraryCmd::Scan {
            music_dir,
            revision: Revision::UNSTAMPED.next().next(),
            mode: ScanMode::Cached,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.event);
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

    let executed = execute(
        LibraryCmd::TagTracks {
            music_dir,
            paths: listed,
            revision: Revision::UNSTAMPED.next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.event);
    });
}

#[rstest]
fn a_scan_after_a_rescan_answers_from_the_cache(dirs: (TempDir, LibraryDirs)) {
    let (directory, paths) = dirs;
    let music_dir = scanned_music_dir(&directory);
    execute(
        LibraryCmd::Scan {
            music_dir: music_dir.clone(),
            revision: Revision::UNSTAMPED.next(),
            mode: ScanMode::Full,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    let executed = execute(
        LibraryCmd::Scan {
            music_dir,
            revision: Revision::UNSTAMPED.next().next(),
            mode: ScanMode::Cached,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.event);
    });
    assert!(executed.warnings.is_empty());
}

#[rstest]
fn a_corrupt_cache_is_noted_and_the_scan_lists_instead(dirs: (TempDir, LibraryDirs)) {
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

    let executed = execute(
        LibraryCmd::Scan {
            music_dir,
            revision: Revision::UNSTAMPED.next(),
            mode: ScanMode::Cached,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert!(matches!(executed.event, Some(LibraryEvent::Listed { .. })));
    assert_eq!(
        executed.warnings,
        vec![LibraryWarning::CacheMissed(CacheMiss::Corrupt)]
    );
}
