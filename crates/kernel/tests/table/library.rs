use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, DiskCmd, Effect, LibraryCmd},
    domain::{
        cue::Cue,
        io_error::IoError,
        model::{Model, ScanStatus},
        overlay::OverlayName,
        player::Player,
        playhead::Playhead,
        revision::Revision,
        speed::Speed,
        time::Moment,
        toast::Toast,
        track::{Tagging, Tags, Track, TrackParts},
    },
    message::{
        BrowseRequest,
        LibraryError,
        LibraryEvent,
        LibrarySubject,
        Message,
        OverlayRequest,
        SearchEdit,
        SearchRequest,
    },
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    effects,
    router::moon_library_scanned,
    track_at,
    update::{send, update},
};

fn paths(tracks: &[Arc<Track>]) -> Vec<PathBuf> {
    tracks
        .iter()
        .map(|track| track.local_path().unwrap().to_path_buf())
        .collect()
}

fn tagged(path: &str, title: &str, seconds: u64) -> Arc<Track> {
    Arc::new(Track::new(TrackParts {
        path: PathBuf::from(path),
        duration: Duration::from_secs(seconds),
        tags: Tags {
            title: Some(title.to_owned()),
            ..Tags::default()
        },
        audio_format: kernel::domain::track::AudioFormat::default(),
    }))
}

fn openings(cmd: Cmd) -> usize {
    effects(cmd)
        .iter()
        .filter(|effect| matches!(effect, Effect::Animate(Cue::LibraryOpened)))
        .count()
}

fn listed_library(paths: &[&str]) -> (Model, Cmd) {
    let mut model = Model::default();
    let tracks = paths.iter().copied().map(track_at).collect();
    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Listed {
            tracks,
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    (model, cmd)
}

fn titles(model: &Model) -> Vec<String> {
    model
        .playlist
        .tracks
        .iter()
        .map(|track| track.display().to_owned())
        .collect()
}

#[test]
fn a_listing_shows_file_stems_in_the_order_it_arrived() {
    let (model, cmd) =
        listed_library(&["/music/a.flac", "/music/b.flac", "/music/c.flac"]);

    assert_eq!(titles(&model), ["a", "b", "c"]);
    assert!(
        model
            .playlist
            .tracks
            .iter()
            .all(|track| track.duration().is_none()
                && matches!(track.tagging(), Tagging::Listed(_)))
    );
    assert_eq!(openings(cmd), 0);
}

#[test]
fn a_listing_reports_how_many_tracks_wait_for_their_tags() {
    let (model, _) = listed_library(&["/music/a.flac", "/music/b.flac"]);

    assert_eq!(model.scan_status, ScanStatus::Tagging { done: 0, total: 2 });
}

#[test]
fn an_empty_listing_opens_the_library_at_once() {
    let (model, cmd) = listed_library(&[]);

    assert_eq!(model.scan_status, ScanStatus::Idle);
    assert_eq!(openings(cmd), 1);
}

fn playing_a(preloaded: Option<Arc<Track>>) -> Model {
    let (mut model, _) = listed_library(&["/music/a.flac", "/music/b.flac"]);
    model.player = Player::Playing {
        track: track_at("/music/a.flac"),
        playhead: Playhead::anchored(
            Duration::ZERO,
            Moment::default(),
            Speed::default(),
        ),
        preloaded,
    };
    model
}

fn tag(model: &mut Model, track: Arc<Track>) -> Cmd {
    update(
        model,
        Message::Library(LibraryEvent::Tagged {
            tracks: vec![track],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap()
}

#[test]
fn a_tagged_chunk_rewrites_its_rows_and_the_playing_track() {
    let mut model = playing_a(None);

    let cmd = tag(&mut model, tagged("/music/a.flac", "Alpha", 200));

    assert_eq!(titles(&model), ["Alpha", "b"]);
    assert_eq!(
        model.player.current().map(|track| (
            track.display().to_owned(),
            track.duration(),
            track.tagging()
        )),
        Some((
            "Alpha".to_owned(),
            Some(Duration::from_secs(200)),
            Tagging::Tagged(Duration::from_secs(200))
        ))
    );
    assert_eq!(model.scan_status, ScanStatus::Tagging { done: 1, total: 2 });
    assert_eq!(openings(cmd), 0);
}

#[test]
fn a_tagged_chunk_rewrites_the_preloaded_track() {
    let mut model = playing_a(Some(track_at("/music/b.flac")));

    drop(tag(&mut model, tagged("/music/b.flac", "Bravo", 300)));

    assert_eq!(titles(&model), ["a", "Bravo"]);
    let Player::Playing { preloaded, .. } = &model.player else {
        panic!("the player must stay playing");
    };
    assert_eq!(
        preloaded.as_ref().map(|track| (
            track.display().to_owned(),
            track.duration(),
            track.tagging()
        )),
        Some((
            "Bravo".to_owned(),
            Some(Duration::from_secs(300)),
            Tagging::Tagged(Duration::from_secs(300))
        ))
    );
}

#[test]
fn a_relist_under_an_open_search_keeps_enter_on_the_highlighted_track() {
    let (mut model, _) = listed_library(&["/music/a.flac", "/music/b.flac"]);
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::Search)),
    );
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Search(SearchRequest::Edit(
            SearchEdit::Char('a'),
        ))),
    );

    send(
        &mut model,
        Message::Library(LibraryEvent::Loaded {
            tracks: vec![track_at("/music/b.flac"), track_at("/music/a.flac")],
            revision: Revision::default(),
        }),
    );
    send(&mut model, Message::Overlay(OverlayRequest::Confirm));

    assert_eq!(
        model
            .player
            .current()
            .map(|track| track.display().to_owned()),
        Some("a".to_owned())
    );
}

#[test]
fn a_tagged_chunk_reaches_the_library_behind_the_playlist() {
    let (mut model, _) = listed_library(&["/music/a.flac", "/music/b.flac"]);

    send(
        &mut model,
        Message::Library(LibraryEvent::Tagged {
            tracks: vec![tagged("/music/b.flac", "Beta", 30)],
            revision: Revision::default(),
        }),
    );

    assert_eq!(
        model.library.as_ref().map(|ready| ready
            .tracks
            .iter()
            .map(|track| track.display().to_owned())
            .collect::<Vec<_>>()),
        Some(vec!["a".to_owned(), "Beta".to_owned()])
    );
}

#[test]
fn the_last_chunk_opens_the_library_once() {
    let (mut model, _) = listed_library(&["/music/a.flac", "/music/b.flac"]);

    let first = update(
        &mut model,
        Message::Library(LibraryEvent::Tagged {
            tracks: vec![tagged("/music/a.flac", "Alpha", 10)],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    let last = update(
        &mut model,
        Message::Library(LibraryEvent::Tagged {
            tracks: vec![tagged("/music/b.flac", "Beta", 20)],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(openings(first), 0);
    assert_eq!(openings(last), 1);
    assert_eq!(model.scan_status, ScanStatus::Idle);
}

#[test]
fn a_chunk_arriving_after_the_last_one_opens_nothing() {
    let (mut model, _) = listed_library(&["/music/a.flac"]);

    let last = update(
        &mut model,
        Message::Library(LibraryEvent::Tagged {
            tracks: vec![tagged("/music/a.flac", "Alpha", 10)],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    let stray = update(
        &mut model,
        Message::Library(LibraryEvent::Tagged {
            tracks: vec![tagged("/music/a.flac", "Alpha", 10)],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(openings(last), 1);
    assert_eq!(openings(stray), 0);
    assert_eq!(model.scan_status, ScanStatus::Idle);
}

fn revision(bumps: u64) -> Revision {
    (0..bumps).fold(Revision::default(), |revision, _| revision.next())
}

fn rescanning_model() -> Model {
    let mut model = Model {
        music_dir: PathBuf::from("/music"),
        ..Model::default()
    };
    let cmd = update(
        &mut model,
        Message::Browse(BrowseRequest::Rescan),
        Moment::default(),
    )
    .unwrap();

    let issued = effects(cmd).iter().find_map(|effect| match effect {
        Effect::Library(LibraryCmd::Scan { revision, .. }) => Some(*revision),
        Effect::Library(_)
        | Effect::Audio(_)
        | Effect::Macos(_)
        | Effect::Config(_)
        | Effect::Remote(_)
        | Effect::WindowColors(_)
        | Effect::Animate(_)
        | Effect::RollShuffle(..)
        | Effect::After { .. }
        | Effect::Restart(_)
        | Effect::Quit => None,
    });
    assert_eq!(issued, Some(revision(1)));
    assert_eq!(model.scan_status, ScanStatus::Scanning);
    model
}

#[rstest]
#[case::the_generation_it_asked_for(1, Some(1), ScanStatus::Idle)]
#[case::a_generation_it_never_asked_for(2, None, ScanStatus::Scanning)]
fn only_the_awaited_scan_generation_lands(
    #[case] bumps: u64,
    #[case] installed: Option<usize>,
    #[case] scan_status: ScanStatus,
) {
    let mut model = rescanning_model();

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Loaded {
            tracks: vec![track_at("/music/a.flac")],
            revision: revision(bumps),
        }),
        Moment::default(),
    );

    assert_eq!(
        model.library.as_ref().map(|ready| ready.tracks.len()),
        installed
    );
    assert_eq!(model.scan_status, scan_status);
    assert_eq!(cmd.map(openings), installed.map(|_| 1).ok_or(Unhandled));
}

#[test]
fn a_listing_asks_for_the_tags_of_everything_it_listed() {
    let mut model = Model {
        music_dir: PathBuf::from("/music"),
        ..Model::default()
    };

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Listed {
            tracks: vec![track_at("/music/a.flac"), track_at("/music/b.flac")],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();

    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn a_listing_from_a_superseded_scan_asks_for_no_tags() {
    let mut model = rescanning_model();

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Listed {
            tracks: vec![track_at("/music/a.flac")],
            revision: Revision::default(),
        }),
        Moment::default(),
    );

    assert_eq!(cmd, Err(Unhandled));
    assert_eq!(model.scan_status, ScanStatus::Scanning);
}

#[test]
fn a_confirmed_trash_keeps_the_track_until_the_disk_answers() {
    let mut model = moon_library_scanned();
    let track = Arc::clone(&model.playlist.tracks[0]);
    model.queue = vec![track.source().clone()];
    let library = paths(&model.library.as_ref().unwrap().tracks);
    let playlist = paths(&model.playlist.tracks);

    let cmd = update(
        &mut model,
        Message::Browse(BrowseRequest::Trash(track.source().clone())),
        Moment::default(),
    );

    assert_eq!(
        cmd.map(effects),
        Ok(vec![Effect::Library(LibraryCmd::Disk(DiskCmd::Trash(
            track.local_path().unwrap().to_path_buf()
        )))])
    );
    let ready = model.library.as_ref().unwrap();
    assert_eq!(paths(&ready.tracks), library);
    assert_eq!(ready.track_indexes.len(), library.len());
    assert_eq!(paths(&model.playlist.tracks), playlist);
    assert_eq!(model.queue, [track.source().clone()]);
}

#[test]
fn a_trashed_answer_removes_the_track() {
    let mut model = moon_library_scanned();
    let track = Arc::clone(&model.playlist.tracks[0]);
    model.queue = vec![track.source().clone()];
    let path = track.local_path().unwrap().to_path_buf();

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Trashed(path.clone())),
        Moment::default(),
    );

    assert_eq!(
        cmd.map(effects),
        Ok(vec![Effect::Animate(Cue::TrackTrashed)])
    );
    let ready = model.library.as_ref().unwrap();
    assert!(!paths(&ready.tracks).contains(&path));
    assert_eq!(ready.track_indexes.len(), ready.tracks.len());
    assert!(model.queue.is_empty());
    assert!(!paths(&model.playlist.tracks).contains(&path));
    assert_eq!(model.playlist.tracks.len(), ready.tracks.len());
}

#[test]
fn a_failed_trash_keeps_the_track_and_raises_a_toast() {
    let mut model = moon_library_scanned();
    let track = Arc::clone(&model.playlist.tracks[0]);
    let library = paths(&model.library.as_ref().unwrap().tracks);
    let playlist = paths(&model.playlist.tracks);
    let error = LibraryError::Disk {
        subject: LibrarySubject::Trash,
        path: track.local_path().unwrap().to_path_buf(),
        error: IoError::Denied,
    };
    send(
        &mut model,
        Message::Browse(BrowseRequest::Trash(track.source().clone())),
    );

    send(
        &mut model,
        Message::Library(LibraryEvent::Error(error.clone())),
    );

    assert_eq!(paths(&model.library.as_ref().unwrap().tracks), library);
    assert_eq!(paths(&model.playlist.tracks), playlist);
    assert_eq!(
        model.workspace.toasts,
        [Toast::error("Library error").with_text(error.to_string())]
    );
}

#[test]
fn a_trashed_answer_for_an_unknown_path_is_refused() {
    let mut model = moon_library_scanned();

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Trashed(PathBuf::from("/m/nowhere.flac"))),
        Moment::default(),
    );

    assert_eq!(cmd, Err(Unhandled));
}

#[test]
fn an_identical_favorites_load_is_refused() {
    let mut model = moon_library_scanned();
    let before = model.clone();

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::FavoritesLoaded(before.favorites.clone())),
        Moment::default(),
    );

    assert_eq!(cmd, Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn an_identical_history_load_is_refused() {
    let mut model = moon_library_scanned();
    let before = model.clone();

    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::HistoryLoaded(before.history.clone())),
        Moment::default(),
    );

    assert_eq!(cmd, Err(Unhandled));
    assert_eq!(model, before);
}
