use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    cmd::{
        AudioCmd,
        Cmd,
        DiskCmd,
        Effect,
        LibraryCmd,
        MacosCmd,
        Playback,
        ScanMode,
        TrackLoad,
    },
    domain::{
        cue::{Cue, PlaybackChange},
        cursor::Cursor,
        direction::Direction,
        favorites::Favorites,
        geometry::Cells,
        history::HistoryEntry,
        index::{TrackIndex, ViewIndex},
        library::{Library, SortKey},
        model::{Model, ScanStatus},
        player::Player,
        playlist::{PlayOrder, PlaylistSource},
        revision::Revision,
        time::Moment,
    },
    message::{AudioEvent, BrowseRequest, Message, PlaybackRequest, QueueRequest},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    bare_track,
    model_with_tracks,
    titled_track,
    update::{send, update},
};

fn browse(model: &mut Model, message: BrowseRequest) -> Cmd {
    update(model, Message::Browse(message), Moment::default()).unwrap()
}

fn queue(model: &mut Model, message: QueueRequest) -> Result<Cmd, Unhandled> {
    update(model, Message::Queue(message), Moment::default())
}

fn queued_refs(model: &Model, queue: &[usize]) -> Vec<kernel::domain::track::TrackRef> {
    queue
        .iter()
        .map(|&row| model.playlist.tracks[row].source().clone())
        .collect()
}

fn paths(tracks: &[Arc<kernel::domain::track::Track>]) -> Vec<PathBuf> {
    tracks
        .iter()
        .map(|track| track.path().to_path_buf())
        .collect()
}

fn browsing(count: usize, row: usize, queue: &[usize]) -> Model {
    let mut model = model_with_tracks(count);
    model.workspace.browse.cursor = Cursor::at(count, row);
    model.queue = queued_refs(&model, queue);
    model
}

struct QueueRow {
    model: Model,
    message: QueueRequest,
    queued: &'static [usize],
    effects: Result<Cmd, Unhandled>,
}

fn queue_changed() -> Result<Cmd, Unhandled> {
    Ok(Cmd::effect(Effect::Animate(Cue::QueueChanged)))
}

fn favorites_saved(paths: &[&str]) -> Cmd {
    let saved: Favorites = paths
        .iter()
        .map(|path| kernel::domain::track::TrackRef::Local(PathBuf::from(path)))
        .collect();
    Cmd::from_iter([
        Effect::Library(LibraryCmd::Disk(DiskCmd::SaveFavorites(saved))),
        Effect::Animate(Cue::FavoriteToggled),
    ])
}

#[rstest]
#[case::enqueue_appends_to_the_back(QueueRow {
    model: browsing(2, 1, &[0]),
    message: QueueRequest::Enqueue,
    queued: &[0, 1],
    effects: queue_changed(),
})]
#[case::enqueue_on_an_empty_playlist_queues_nothing(QueueRow {
    model: Model::default(),
    message: QueueRequest::Enqueue,
    queued: &[],
    effects: Err(Unhandled),
})]
#[case::enqueue_of_an_already_queued_track_takes_it_back_out(QueueRow {
    model: browsing(2, 1, &[1, 0]),
    message: QueueRequest::Enqueue,
    queued: &[0],
    effects: queue_changed(),
})]
#[case::play_next_inserts_at_the_front(QueueRow {
    model: browsing(2, 1, &[0]),
    message: QueueRequest::PlayNext,
    queued: &[1, 0],
    effects: queue_changed(),
})]
#[case::play_next_moves_an_already_queued_track_to_the_front(QueueRow {
    model: browsing(3, 2, &[0, 2, 1]),
    message: QueueRequest::PlayNext,
    queued: &[2, 0, 1],
    effects: queue_changed(),
})]
#[case::play_next_of_the_front_track_is_refused(QueueRow {
    model: browsing(2, 1, &[1, 0]),
    message: QueueRequest::PlayNext,
    queued: &[1, 0],
    effects: Err(Unhandled),
})]
#[case::play_next_on_an_empty_playlist_queues_nothing(QueueRow {
    model: Model::default(),
    message: QueueRequest::PlayNext,
    queued: &[],
    effects: Err(Unhandled),
})]
#[case::dequeue_removes_the_single_occurrence(QueueRow {
    model: browsing(2, 1, &[1, 0]),
    message: QueueRequest::Dequeue,
    queued: &[0],
    effects: queue_changed(),
})]
#[case::dequeue_of_an_unqueued_selection_changes_nothing(QueueRow {
    model: browsing(2, 1, &[0]),
    message: QueueRequest::Dequeue,
    queued: &[0],
    effects: Err(Unhandled),
})]
#[case::dequeue_on_an_empty_playlist_changes_nothing(QueueRow {
    model: browsing(0, 0, &[]),
    message: QueueRequest::Dequeue,
    queued: &[],
    effects: Err(Unhandled),
})]
#[case::move_up_swaps_with_the_predecessor(QueueRow {
    model: browsing(3, 1, &[0, 1, 2]),
    message: QueueRequest::MoveInQueue(Direction::Previous),
    queued: &[1, 0, 2],
    effects: queue_changed(),
})]
#[case::move_down_swaps_with_the_successor(QueueRow {
    model: browsing(3, 0, &[0, 1, 2]),
    message: QueueRequest::MoveInQueue(Direction::Next),
    queued: &[1, 0, 2],
    effects: queue_changed(),
})]
#[case::move_up_at_the_front_changes_nothing(QueueRow {
    model: browsing(2, 0, &[0, 1]),
    message: QueueRequest::MoveInQueue(Direction::Previous),
    queued: &[0, 1],
    effects: Err(Unhandled),
})]
#[case::move_down_at_the_back_changes_nothing(QueueRow {
    model: browsing(2, 1, &[0, 1]),
    message: QueueRequest::MoveInQueue(Direction::Next),
    queued: &[0, 1],
    effects: Err(Unhandled),
})]
#[case::move_of_an_unqueued_selection_changes_nothing(QueueRow {
    model: browsing(2, 1, &[0]),
    message: QueueRequest::MoveInQueue(Direction::Previous),
    queued: &[0],
    effects: Err(Unhandled),
})]
#[case::move_on_an_empty_playlist_changes_nothing(QueueRow {
    model: browsing(0, 0, &[]),
    message: QueueRequest::MoveInQueue(Direction::Previous),
    queued: &[],
    effects: Err(Unhandled),
})]
fn queue_row(#[case] row: QueueRow) {
    let QueueRow {
        mut model,
        message,
        queued,
        effects,
    } = row;
    let seen = queue(&mut model, message);
    assert_eq!(model.queue, queued_refs(&model, queued));
    assert_eq!(seen, effects);
}

struct CursorRow {
    tracks: usize,
    from: usize,
    visible_rows: Cells,
    message: BrowseRequest,
    expected: usize,
    effects: Result<Cmd, Unhandled>,
}

#[rstest]
#[case::page_down_moves_by_the_reported_rows(CursorRow {
    tracks: 20,
    from: 0,
    visible_rows: Cells(5),
    message: BrowseRequest::PageBy(Direction::Next),
    expected: 5,
    effects: Ok(Cmd::none()),
})]
#[case::page_down_clamps_at_the_last_track(CursorRow {
    tracks: 8,
    from: 6,
    visible_rows: Cells(5),
    message: BrowseRequest::PageBy(Direction::Next),
    expected: 7,
    effects: Ok(Cmd::none()),
})]
#[case::page_up_moves_by_the_reported_rows(CursorRow {
    tracks: 20,
    from: 8,
    visible_rows: Cells(5),
    message: BrowseRequest::PageBy(Direction::Previous),
    expected: 3,
    effects: Ok(Cmd::none()),
})]
#[case::page_up_clamps_at_the_first_track(CursorRow {
    tracks: 20,
    from: 2,
    visible_rows: Cells(5),
    message: BrowseRequest::PageBy(Direction::Previous),
    expected: 0,
    effects: Ok(Cmd::none()),
})]
#[case::page_down_with_no_rows_reported_is_refused(CursorRow {
    tracks: 20,
    from: 0,
    visible_rows: Cells(0),
    message: BrowseRequest::PageBy(Direction::Next),
    expected: 0,
    effects: Err(Unhandled),
})]
#[case::page_down_on_an_empty_playlist_is_refused(CursorRow {
    tracks: 0,
    from: 0,
    visible_rows: Cells(5),
    message: BrowseRequest::PageBy(Direction::Next),
    expected: 0,
    effects: Err(Unhandled),
})]
#[case::page_up_on_an_empty_playlist_is_refused(CursorRow {
    tracks: 0,
    from: 0,
    visible_rows: Cells(5),
    message: BrowseRequest::PageBy(Direction::Previous),
    expected: 0,
    effects: Err(Unhandled),
})]
#[case::cursor_by_on_an_empty_playlist_is_refused(CursorRow {
    tracks: 0,
    from: 0,
    visible_rows: Cells(5),
    message: BrowseRequest::CursorBy { rows: 1 },
    expected: 0,
    effects: Err(Unhandled),
})]
fn cursor_row(#[case] row: CursorRow) {
    let mut model = browsing(row.tracks, row.from, &[]);
    model.workspace.visible_rows = row.visible_rows;
    let effects = update(&mut model, Message::Browse(row.message), Moment::default());
    assert_eq!(model.workspace.browse.selected().get(), row.expected);
    assert_eq!(effects, row.effects);
}

#[rstest]
#[case::a_single_row(Cells(1), 1)]
#[case::a_full_page(Cells(20), 20)]
fn page_by_uses_the_stored_viewport(
    #[case] visible_rows: Cells,
    #[case] expected: usize,
) {
    let mut model = browsing(30, 0, &[]);
    model.workspace.visible_rows = visible_rows;
    send(
        &mut model,
        Message::Browse(BrowseRequest::PageBy(Direction::Next)),
    );
    assert_eq!(model.workspace.browse.selected().get(), expected);
}

fn unsorted_library() -> Model {
    let all = vec![
        titled_track("/music/c.flac", "C", "Charlie"),
        titled_track("/music/a.flac", "A", "Alpha"),
        titled_track("/music/b.flac", "B", "Bravo"),
    ];
    let view = (0..all.len()).map(TrackIndex::new).collect();
    Model {
        library: Some(Library { tracks: all, view }),
        ..Default::default()
    }
}

#[rstest]
#[case::added_cycles_to_favorites(SortKey::Added, SortKey::Favorites)]
#[case::favorites_cycles_back_to_artist(SortKey::Favorites, SortKey::Artist)]
fn cycle_sort_walks_the_keys(#[case] from: SortKey, #[case] expected: SortKey) {
    let mut model = Model::default();
    model.workspace.browse.sort = from;
    send(&mut model, Message::Browse(BrowseRequest::CycleSort));
    assert_eq!(model.workspace.browse.sort, expected);
}

#[test]
fn cycle_sort_to_artist_reorders_the_view_and_the_playlist_under_it() {
    let mut model = unsorted_library();
    send(&mut model, Message::Browse(BrowseRequest::CycleSort));
    send(&mut model, Message::Browse(BrowseRequest::CycleSort));

    assert_eq!(model.workspace.browse.sort, SortKey::Artist);
    let sorted = vec![
        PathBuf::from("/music/a.flac"),
        PathBuf::from("/music/b.flac"),
        PathBuf::from("/music/c.flac"),
    ];
    let view: Vec<PathBuf> = model
        .library
        .iter()
        .flat_map(Library::view_tracks)
        .map(|(_, track)| track.path().to_path_buf())
        .collect();
    assert_eq!(view, sorted);
    assert_eq!(paths(&model.playlist.tracks), sorted);
    let all = model.library.as_ref().unwrap().tracks.len();
    assert_eq!(all, 3);
}

fn view_paths(model: &Model) -> Vec<PathBuf> {
    model
        .library
        .iter()
        .flat_map(Library::view_tracks)
        .map(|(_, track)| track.path().to_path_buf())
        .collect()
}

fn scan_order() -> Vec<PathBuf> {
    ["c", "a", "b"]
        .map(|stem| PathBuf::from(format!("/music/{stem}.flac")))
        .to_vec()
}

#[test]
fn cycle_sort_to_added_restores_the_scan_order() {
    let mut model = unsorted_library();
    model.workspace.browse.sort = SortKey::Year;
    model.library.as_mut().unwrap().view = [1, 2, 0].map(TrackIndex::new).to_vec();

    send(&mut model, Message::Browse(BrowseRequest::CycleSort));

    assert_eq!(model.workspace.browse.sort, SortKey::Added);
    assert_eq!(view_paths(&model), scan_order());
    assert_eq!(paths(&model.playlist.tracks), scan_order());
}

#[test]
fn a_rescan_keeps_the_view_in_the_chosen_sort_order() {
    let mut model = Model::default();
    model.workspace.browse.sort = SortKey::Artist;

    send(
        &mut model,
        Message::Library(kernel::message::LibraryEvent::Loaded {
            tracks: vec![
                titled_track("/music/c.flac", "C", "Charlie"),
                titled_track("/music/a.flac", "A", "Alpha"),
                titled_track("/music/b.flac", "B", "Bravo"),
            ],
            revision: Revision::default(),
        }),
    );

    let sorted = vec![
        PathBuf::from("/music/a.flac"),
        PathBuf::from("/music/b.flac"),
        PathBuf::from("/music/c.flac"),
    ];
    assert_eq!(view_paths(&model), sorted);
    assert_eq!(paths(&model.playlist.tracks), sorted);
}

fn named_playlist(model: &mut Model) -> Vec<PathBuf> {
    model.playlist_source = PlaylistSource::Named;
    model.playlist.tracks = vec![titled_track("/elsewhere/one.flac", "One", "")];
    paths(&model.playlist.tracks)
}

#[test]
fn cycle_sort_leaves_a_named_playlist_alone() {
    let mut model = unsorted_library();
    let named = named_playlist(&mut model);

    send(&mut model, Message::Browse(BrowseRequest::CycleSort));

    assert_eq!(paths(&model.playlist.tracks), named);
}

#[test]
fn trash_leaves_a_named_playlist_alone() {
    let mut model = scanned(&["/music/a.flac", "/music/b.flac"]);
    let source = model.library.as_ref().unwrap().tracks[0].source().clone();
    let named = named_playlist(&mut model);

    send(&mut model, Message::Browse(BrowseRequest::Trash(source)));

    assert_eq!(paths(&model.playlist.tracks), named);
}

#[test]
fn cycle_sort_collapses_a_stale_shuffle_order_to_pending() {
    let mut model = unsorted_library();
    model.playlist.play_order =
        PlayOrder::Shuffle([1, 0, 2].map(ViewIndex::new).to_vec());
    send(&mut model, Message::Browse(BrowseRequest::CycleSort));
    assert_eq!(model.playlist.play_order, PlayOrder::ShufflePending);
}

#[test]
fn toggle_favorite_adds_then_removes_the_selected_track() {
    let mut model = browsing(2, 1, &[]);
    let selected = kernel::domain::track::TrackRef::Local("/tmp/track1.flac".into());
    let other = kernel::domain::track::TrackRef::Local("/tmp/track0.flac".into());

    let effects = browse(&mut model, BrowseRequest::ToggleFavorite);
    assert!(model.favorites.is_favorite(&selected));
    assert!(!model.favorites.is_favorite(&other));
    assert_eq!(effects, favorites_saved(&["/tmp/track1.flac"]));

    let untoggled = browse(&mut model, BrowseRequest::ToggleFavorite);
    assert!(!model.favorites.is_favorite(&selected));
    assert_eq!(untoggled, favorites_saved(&[]));
}

#[test]
fn toggle_favorite_on_an_empty_playlist_is_refused() {
    let mut model = Model::default();
    let effects = update(
        &mut model,
        Message::Browse(BrowseRequest::ToggleFavorite),
        Moment::default(),
    );
    assert!(model.favorites.is_empty());
    assert_eq!(effects, Err(Unhandled));
}

#[test]
fn play_selected_jumps_the_playlist_and_starts_the_track() {
    let mut model = browsing(2, 1, &[]);
    let effects = browse(&mut model, BrowseRequest::PlaySelected);

    assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(1)));
    assert!(matches!(
        &model.player,
        Player::Loading(track) if track.path() == Path::new("/tmp/track1.flac")
    ));
    let track = bare_track(1);
    assert_eq!(
        effects,
        Cmd::from_iter([
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::Load(TrackLoad {
                path: track.path().to_path_buf(),
                gain: None,
                revision: Revision::default().next(),
            })),
            Effect::Library(LibraryCmd::Disk(DiskCmd::AppendHistory(
                HistoryEntry::from_track(&track, Moment::default()),
            ))),
            Effect::Macos(MacosCmd::NowPlaying(Some(track))),
            Effect::Audio(AudioCmd::SetPlayback(Playback::Playing)),
            Effect::Macos(MacosCmd::SetPlayback(Playback::Playing)),
            Effect::Animate(Cue::TrackChanged),
            Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Play)),
        ])
    );
}

#[test]
fn play_selected_retires_the_stream_the_media_key_started() {
    let mut model = browsing(3, 2, &[]);
    send(&mut model, Message::Playback(PlaybackRequest::Play));
    send(&mut model, Message::Audio(AudioEvent::Loaded(None)));

    let picked = browse(&mut model, BrowseRequest::PlaySelected);
    let audio: Vec<&Effect> = picked
        .effects()
        .filter(|effect| matches!(effect, Effect::Audio(_)))
        .collect();

    let retired = audio
        .iter()
        .position(|effect| matches!(effect, Effect::Audio(AudioCmd::Stop)));
    let loaded = audio
        .iter()
        .position(|effect| matches!(effect, Effect::Audio(AudioCmd::Load(_))));
    assert!(
        matches!((retired, loaded), (Some(retired), Some(loaded)) if retired < loaded),
        "the media key's stream must be retired before the picked row loads, \
         or both sound at once, got {audio:?}"
    );
}

#[test]
fn play_selected_on_an_empty_playlist_starts_nothing() {
    let mut model = Model::default();
    let result = update(
        &mut model,
        Message::Browse(BrowseRequest::PlaySelected),
        Moment::default(),
    );
    assert_eq!(model.playlist.playing_index(), None);
    assert_eq!(model.player, Player::Stopped);
    assert_eq!(result, Err(Unhandled));
}

#[test]
fn rescan_asks_once_until_the_scan_lands() {
    let mut model = Model {
        music_dir: PathBuf::from("/music"),
        ..Default::default()
    };

    let effects = browse(&mut model, BrowseRequest::FullScan);
    assert_eq!(
        effects,
        Cmd::effect(Effect::Library(LibraryCmd::Scan {
            music_dir: PathBuf::from("/music"),
            revision: Revision::default().next(),
            mode: ScanMode::Full,
        }))
    );
    assert_eq!(model.scan_status, ScanStatus::Scanning);

    let repeated = update(
        &mut model,
        Message::Browse(BrowseRequest::FullScan),
        Moment::default(),
    );
    assert_eq!(repeated, Err(Unhandled));
    assert_eq!(model.scan_status, ScanStatus::Scanning);
}

fn scanned(paths: &[&str]) -> Model {
    let all: Vec<_> = paths
        .iter()
        .map(|path| titled_track(path, path, ""))
        .collect();
    let view = (0..all.len()).map(TrackIndex::new).collect();
    let mut model = Model {
        library: Some(Library {
            tracks: all.clone(),
            view,
        }),
        ..Default::default()
    };
    model.playlist.tracks = all;
    model
}

#[test]
fn trash_removes_the_track_everywhere_and_asks_for_the_file_to_go() {
    let mut model = scanned(&["/music/a.flac", "/music/b.flac"]);
    let source = model.playlist.tracks[0].source().clone();
    let effects = browse(&mut model, BrowseRequest::Trash(source));

    let left = vec![PathBuf::from("/music/b.flac")];
    let all = paths(&model.library.as_ref().unwrap().tracks);
    assert_eq!(all, left);
    let view: Vec<PathBuf> = model
        .library
        .iter()
        .flat_map(Library::view_tracks)
        .map(|(_, track)| track.path().to_path_buf())
        .collect();
    assert_eq!(view, left);
    assert_eq!(paths(&model.playlist.tracks), left);
    assert_eq!(
        effects,
        Cmd::from_iter([
            Effect::Library(LibraryCmd::Disk(DiskCmd::Trash(PathBuf::from(
                "/music/a.flac"
            )))),
            Effect::Animate(Cue::TrackDeleted),
        ])
    );
}

#[test]
fn trash_remaps_the_queue_and_drops_the_deleted_entry() {
    let mut model = scanned(&["/music/a.flac", "/music/b.flac", "/music/c.flac"]);
    model.queue = queued_refs(&model, &[2, 0]);
    let source = model.playlist.tracks[0].source().clone();

    send(&mut model, Message::Browse(BrowseRequest::Trash(source)));

    assert_eq!(
        paths(&model.playlist.tracks),
        vec![
            PathBuf::from("/music/b.flac"),
            PathBuf::from("/music/c.flac"),
        ]
    );
    assert_eq!(model.queue, queued_refs(&model, &[1]));
}

#[test]
fn trash_shrinks_the_browse_cursor_with_the_playlist() {
    let mut model = scanned(&["/music/a.flac", "/music/b.flac"]);
    model.workspace.browse.cursor = Cursor::at(2, 1);
    let source = model.playlist.tracks[0].source().clone();

    send(&mut model, Message::Browse(BrowseRequest::Trash(source)));

    assert_eq!(model.workspace.browse.cursor, Cursor::at(1, 0));
}

#[rstest]
#[case::missing_from_the_library(scanned(&["/music/a.flac"]))]
#[case::without_a_library(Model::default())]
fn trash_of_an_unknown_track_is_refused(#[case] mut model: Model) {
    let before = format!("{model:?}");
    let gone =
        kernel::domain::track::TrackRef::Local(PathBuf::from("/music/gone.flac"));

    let refused = update(
        &mut model,
        Message::Browse(BrowseRequest::Trash(gone)),
        Moment::default(),
    );

    assert_eq!(refused.err(), Some(Unhandled));
    assert_eq!(format!("{model:?}"), before);
}

fn listed(paths: &[&str]) -> Message {
    Message::Library(kernel::message::LibraryEvent::Listed {
        tracks: paths
            .iter()
            .map(|path| Arc::new(kernel::domain::track::Track::listed(Path::new(path))))
            .collect(),
        revision: Revision::default(),
    })
}

#[test]
fn a_rescan_under_the_confirm_overlay_trashes_the_same_file() {
    let mut model = Model::default();
    send(&mut model, listed(&["/music/a.flac", "/music/b.flac"]));
    send(
        &mut model,
        Message::Browse(BrowseRequest::CursorBy { rows: 1 }),
    );
    send(
        &mut model,
        Message::Overlay(kernel::message::OverlayRequest::Open(
            kernel::domain::overlay::OverlayName::ConfirmDelete,
        )),
    );
    send(
        &mut model,
        listed(&["/music/0.flac", "/music/a.flac", "/music/b.flac"]),
    );

    let confirmed = update(
        &mut model,
        Message::Overlay(kernel::message::OverlayRequest::Confirm),
        Moment::default(),
    )
    .unwrap();

    assert!(confirmed.effects().any(|effect| *effect
        == Effect::Library(LibraryCmd::Disk(DiskCmd::Trash(PathBuf::from(
            "/music/b.flac"
        ))))));
    assert_eq!(
        paths(&model.playlist.tracks),
        [
            PathBuf::from("/music/0.flac"),
            PathBuf::from("/music/a.flac")
        ]
    );
}
