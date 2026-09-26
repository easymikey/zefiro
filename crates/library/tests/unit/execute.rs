use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    AudioFormat,
    HistoryEntry,
    LibraryCmd,
    LibraryFact,
    Tags,
    Track,
    domain::{Revision, UnixSeconds},
    playlist::PlaylistFileName,
};
use library::{CacheMiss, LibraryNote, LibraryPaths, execute};

const FIXTURE_LENGTH: Duration = Duration::from_secs(180);
const DECODABLE: &[&str] = &["flac", "wav"];

fn library_paths(root: &std::path::Path) -> LibraryPaths {
    LibraryPaths {
        cache: root.join("cache"),
        data: root.join("data"),
        playlists: root.join("playlists"),
    }
}

fn tmp_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            r"(/private)?/var/folders/[^/]+/[^/]+/T/\.tmp[A-Za-z0-9]+",
            "[tmp]",
        ),
        (r"/tmp/\.tmp[A-Za-z0-9]+", "[tmp]"),
    ]
}

fn track(path: &str) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(path)
            .duration(FIXTURE_LENGTH)
            .tags(Tags {
                title: Some("Song".to_string()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    )
}

#[test]
fn append_history_writes_an_entry_and_replies_with_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());

    let executed = execute(
        LibraryCmd::AppendHistory {
            track: track("/music/song.flac"),
            at: UnixSeconds::UNSTAMPED,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(executed.fact, None);
    assert!(paths.data.join("history.jsonl").is_file());
}

#[test]
fn save_favorites_writes_the_file_and_replies_with_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let mut saved = HashSet::new();
    saved.insert(PathBuf::from("/music/a.flac"));

    let executed = execute(
        LibraryCmd::SaveFavorites(Arc::new(saved)),
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(executed.fact, None);
    assert!(paths.data.join("favorites.json").is_file());
}

#[test]
fn load_favorites_replies_with_the_saved_set() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let mut saved = HashSet::new();
    saved.insert(PathBuf::from("/music/a.flac"));
    execute(
        LibraryCmd::SaveFavorites(Arc::new(saved)),
        &paths,
        DECODABLE,
    )
    .unwrap();

    let executed = execute(LibraryCmd::LoadFavorites, &paths, DECODABLE).unwrap();

    insta::assert_debug_snapshot!(executed.fact);
}

#[test]
fn trash_of_a_missing_file_replies_with_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let missing = directory.path().join("never-existed.flac");

    let executed = execute(LibraryCmd::Trash(missing), &paths, DECODABLE).unwrap();

    assert_eq!(executed.fact, None);
}

#[test]
fn load_history_replies_with_the_appended_entry() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
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

    match executed.fact {
        Some(LibraryFact::HistoryLoaded(entries)) => {
            let listed: Vec<PathBuf> = entries
                .iter()
                .map(|entry: &HistoryEntry| entry.path.clone())
                .collect();
            assert_eq!(listed, vec![PathBuf::from("/music/song.flac")]);
        }
        other => panic!("expected HistoryLoaded, got {other:?}"),
    }
    assert!(executed.notes.is_empty());
}

#[test]
fn save_playlist_writes_under_the_sanitised_name() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());

    let executed = execute(
        LibraryCmd::SavePlaylist {
            name: PlaylistFileName::new("My Mix").unwrap(),
            tracks: vec![track("/music/song.flac")],
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(executed.fact, None);
    assert!(paths.playlists.join("My Mix.m3u8").is_file());
}

const TONE: &[u8] = include_bytes!("../fixtures/tone.wav");

fn scanned_root(directory: &tempfile::TempDir) -> PathBuf {
    let root = directory.path().join("music");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.wav"), TONE).unwrap();
    root
}

#[test]
fn rescan_replies_with_the_tracks_it_found_under_the_revision_it_was_given() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);

    let executed = execute(
        LibraryCmd::Rescan {
            root,
            revision: Revision::UNSTAMPED.next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.fact);
    });
}

#[test]
fn scan_library_lists_placeholder_tracks_under_the_revision_it_was_given() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);

    let executed = execute(
        LibraryCmd::ScanLibrary {
            root,
            revision: Revision::UNSTAMPED.next().next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.fact);
    });
}

#[test]
fn tag_tracks_answers_every_listed_path_under_the_revision_it_was_given() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);
    let listed = vec![root.join("a.wav"), root.join("never-existed.flac")];

    let executed = execute(
        LibraryCmd::TagTracks {
            root,
            paths: listed,
            revision: Revision::UNSTAMPED.next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.fact);
    });
}

#[test]
fn a_scan_after_a_rescan_answers_from_the_cache() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);
    execute(
        LibraryCmd::Rescan {
            root: root.clone(),
            revision: Revision::UNSTAMPED.next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    let executed = execute(
        LibraryCmd::ScanLibrary {
            root,
            revision: Revision::UNSTAMPED.next().next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(executed.fact);
    });
    assert!(executed.notes.is_empty());
}

#[test]
fn a_corrupt_cache_is_noted_and_the_scan_lists_instead() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);
    std::fs::create_dir_all(&paths.cache).unwrap();
    std::fs::write(
        paths.cache.join("library.dir"),
        root.to_string_lossy().as_bytes(),
    )
    .unwrap();
    std::fs::write(
        paths.cache.join("library.bin"),
        [5u8, 0xDE, 0xAD, 0xBE, 0xEF],
    )
    .unwrap();

    let executed = execute(
        LibraryCmd::ScanLibrary {
            root,
            revision: Revision::UNSTAMPED.next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert!(matches!(executed.fact, Some(LibraryFact::Listed { .. })));
    assert_eq!(
        executed.notes,
        vec![LibraryNote::CacheMissed(CacheMiss::Corrupt)]
    );
}
