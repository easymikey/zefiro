use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    AudioFormat,
    HistoryEntry,
    LibraryCmd,
    LoadedRequest,
    Message,
    Tags,
    Track,
    domain::Revision,
};
use library::{LibraryPaths, execute};

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

    let reply = execute(
        LibraryCmd::AppendHistory {
            track: track("/music/song.flac"),
            revision: Revision::UNSTAMPED,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(reply, None);
    assert!(paths.data.join("history.jsonl").is_file());
}

#[test]
fn save_favorites_writes_the_file_and_replies_with_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let mut saved = HashSet::new();
    saved.insert(PathBuf::from("/music/a.flac"));

    let reply = execute(
        LibraryCmd::SaveFavorites(Arc::new(saved)),
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(reply, None);
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

    let reply = execute(LibraryCmd::LoadFavorites, &paths, DECODABLE).unwrap();

    insta::assert_debug_snapshot!(reply);
}

#[test]
fn trash_of_a_missing_file_replies_with_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let missing = directory.path().join("never-existed.flac");

    let reply = execute(LibraryCmd::Trash(missing), &paths, DECODABLE).unwrap();

    assert_eq!(reply, None);
}

#[test]
fn load_history_replies_with_the_appended_entry() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    execute(
        LibraryCmd::AppendHistory {
            track: track("/music/song.flac"),
            revision: Revision::UNSTAMPED,
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    let reply =
        execute(LibraryCmd::LoadHistory { limit: 10 }, &paths, DECODABLE).unwrap();

    match reply {
        Some(Message::Loaded(LoadedRequest::HistoryLoaded(entries))) => {
            let listed: Vec<PathBuf> = entries
                .iter()
                .map(|entry: &HistoryEntry| entry.path.clone())
                .collect();
            assert_eq!(listed, vec![PathBuf::from("/music/song.flac")]);
        }
        other => panic!("expected HistoryLoaded, got {other:?}"),
    }
}

#[test]
fn save_playlist_writes_under_the_sanitised_name() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());

    let reply = execute(
        LibraryCmd::SavePlaylist {
            name: "My Mix".to_string(),
            tracks: vec![track("/music/song.flac")],
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(reply, None);
    assert!(paths.playlists.join("My Mix.m3u8").is_file());
}

#[test]
fn save_playlist_with_a_rejected_name_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());

    let reply = execute(
        LibraryCmd::SavePlaylist {
            name: "...".to_string(),
            tracks: vec![track("/music/song.flac")],
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    assert_eq!(reply, None);
    assert!(!paths.playlists.exists());
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

    let reply = execute(
        LibraryCmd::Rescan {
            root,
            revision: Revision::UNSTAMPED.next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(reply);
    });
}

#[test]
fn scan_library_lists_placeholder_tracks_under_the_revision_it_was_given() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);

    let reply = execute(
        LibraryCmd::ScanLibrary {
            root,
            revision: Revision::UNSTAMPED.next().next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(reply);
    });
}

#[test]
fn tag_tracks_answers_every_listed_path_under_the_revision_it_was_given() {
    let directory = tempfile::tempdir().unwrap();
    let paths = library_paths(directory.path());
    let root = scanned_root(&directory);
    let listed = vec![root.join("a.wav"), root.join("never-existed.flac")];

    let reply = execute(
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
        insta::assert_debug_snapshot!(reply);
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

    let reply = execute(
        LibraryCmd::ScanLibrary {
            root,
            revision: Revision::UNSTAMPED.next().next(),
        },
        &paths,
        DECODABLE,
    )
    .unwrap();

    insta::with_settings!({ filters => tmp_filters() }, {
        insta::assert_debug_snapshot!(reply);
    });
}
