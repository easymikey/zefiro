use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kernel::{
    AudioCmd,
    AudioEvent,
    BrowseRequest,
    Cmd,
    Cue,
    Effect,
    LibraryCmd,
    Message,
    Model,
    Moment,
    NowPlaying,
    Playback,
    PlaybackChange,
    PlaybackRequest,
    Player,
    SystemCmd,
    domain::{
        Cursor,
        Loaded,
        Nudge,
        PlaylistIndex,
        Revision,
        ScanStatus,
        TrackIndex,
        UnixSeconds,
    },
    library::{Library, SortKey},
    playlist::PlayOrder,
    update::update,
};
use rstest::rstest;

use crate::support::{bare_track, model_with_tracks, titled_track};

fn browse(model: &mut Model, message: BrowseRequest) -> Cmd {
    update(model, Message::Browse(message), Moment::default()).unwrap()
}

fn indices(queue: &[usize]) -> Vec<PlaylistIndex> {
    queue.iter().copied().map(PlaylistIndex::new).collect()
}

fn paths(tracks: &[Arc<kernel::Track>]) -> Vec<PathBuf> {
    tracks
        .iter()
        .map(|track| track.path().to_path_buf())
        .collect()
}

fn browsing(count: usize, row: usize, queue: &[usize]) -> Model {
    let mut model = model_with_tracks(count);
    model.workspace.browse.cursor = Cursor::with_len(count).at(row);
    model.queue = indices(queue);
    model
}

struct QueueRow {
    model: Model,
    message: BrowseRequest,
    queued: &'static [usize],
    effects: Cmd,
}

fn queue_changed() -> Cmd {
    Cmd::One(Effect::Animate(Cue::QueueChanged))
}

fn favorites_saved(paths: &[&str]) -> Cmd {
    let saved: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
    Cmd::Batch(vec![
        Effect::Library(LibraryCmd::SaveFavorites(Arc::new(saved))),
        Effect::Animate(Cue::FavoriteToggled),
    ])
}

#[rstest]
#[case::enqueue_appends_to_the_back(QueueRow {
    model: browsing(2, 1, &[0]),
    message: BrowseRequest::Enqueue,
    queued: &[0, 1],
    effects: queue_changed(),
})]
#[case::enqueue_on_an_empty_playlist_queues_nothing(QueueRow {
    model: Model::default(),
    message: BrowseRequest::Enqueue,
    queued: &[],
    effects: Cmd::None,
})]
#[case::enqueue_of_an_already_queued_track_takes_it_back_out(QueueRow {
    model: browsing(2, 1, &[1, 0]),
    message: BrowseRequest::Enqueue,
    queued: &[0],
    effects: queue_changed(),
})]
#[case::play_next_inserts_at_the_front(QueueRow {
    model: browsing(2, 1, &[0]),
    message: BrowseRequest::PlayNext,
    queued: &[1, 0],
    effects: Cmd::None,
})]
#[case::play_next_moves_an_already_queued_track_to_the_front(QueueRow {
    model: browsing(3, 2, &[0, 2, 1]),
    message: BrowseRequest::PlayNext,
    queued: &[2, 0, 1],
    effects: Cmd::None,
})]
#[case::play_next_on_an_empty_playlist_queues_nothing(QueueRow {
    model: Model::default(),
    message: BrowseRequest::PlayNext,
    queued: &[],
    effects: Cmd::None,
})]
#[case::dequeue_removes_the_single_occurrence(QueueRow {
    model: browsing(2, 1, &[1, 0]),
    message: BrowseRequest::Dequeue,
    queued: &[0],
    effects: Cmd::None,
})]
#[case::dequeue_of_an_unqueued_selection_changes_nothing(QueueRow {
    model: browsing(2, 1, &[0]),
    message: BrowseRequest::Dequeue,
    queued: &[0],
    effects: Cmd::None,
})]
#[case::dequeue_on_an_empty_playlist_changes_nothing(QueueRow {
    model: browsing(0, 0, &[0]),
    message: BrowseRequest::Dequeue,
    queued: &[0],
    effects: Cmd::None,
})]
#[case::move_up_swaps_with_the_predecessor(QueueRow {
    model: browsing(3, 1, &[0, 1, 2]),
    message: BrowseRequest::MoveInQueue(Nudge::Up),
    queued: &[1, 0, 2],
    effects: Cmd::None,
})]
#[case::move_down_swaps_with_the_successor(QueueRow {
    model: browsing(3, 0, &[0, 1, 2]),
    message: BrowseRequest::MoveInQueue(Nudge::Down),
    queued: &[1, 0, 2],
    effects: Cmd::None,
})]
#[case::move_up_at_the_front_changes_nothing(QueueRow {
    model: browsing(2, 0, &[0, 1]),
    message: BrowseRequest::MoveInQueue(Nudge::Up),
    queued: &[0, 1],
    effects: Cmd::None,
})]
#[case::move_down_at_the_back_changes_nothing(QueueRow {
    model: browsing(2, 1, &[0, 1]),
    message: BrowseRequest::MoveInQueue(Nudge::Down),
    queued: &[0, 1],
    effects: Cmd::None,
})]
#[case::move_of_an_unqueued_selection_changes_nothing(QueueRow {
    model: browsing(2, 1, &[0]),
    message: BrowseRequest::MoveInQueue(Nudge::Up),
    queued: &[0],
    effects: Cmd::None,
})]
#[case::move_on_an_empty_playlist_changes_nothing(QueueRow {
    model: browsing(0, 0, &[0]),
    message: BrowseRequest::MoveInQueue(Nudge::Up),
    queued: &[0],
    effects: Cmd::None,
})]
fn queue_row(#[case] row: QueueRow) {
    let QueueRow {
        mut model,
        message,
        queued,
        effects,
    } = row;
    let seen = browse(&mut model, message);
    assert_eq!(model.queue, indices(queued));
    assert_eq!(seen, effects);
}

struct CursorRow {
    tracks: usize,
    from: usize,
    visible_rows: usize,
    message: BrowseRequest,
    expected: usize,
}

#[rstest]
#[case::cursor_to_lands_on_the_row(CursorRow {
    tracks: 2,
    from: 0,
    visible_rows: 0,
    message: BrowseRequest::CursorTo(PlaylistIndex::new(1)),
    expected: 1,
})]
#[case::cursor_to_past_the_end_clamps(CursorRow {
    tracks: 2,
    from: 0,
    visible_rows: 0,
    message: BrowseRequest::CursorTo(PlaylistIndex::new(99)),
    expected: 1,
})]
#[case::cursor_to_on_an_empty_playlist_stays_at_zero(CursorRow {
    tracks: 0,
    from: 0,
    visible_rows: 0,
    message: BrowseRequest::CursorTo(PlaylistIndex::new(0)),
    expected: 0,
})]
#[case::page_down_moves_by_the_reported_rows(CursorRow {
    tracks: 20,
    from: 0,
    visible_rows: 5,
    message: BrowseRequest::PageBy(Nudge::Down),
    expected: 5,
})]
#[case::page_down_clamps_at_the_last_track(CursorRow {
    tracks: 8,
    from: 6,
    visible_rows: 5,
    message: BrowseRequest::PageBy(Nudge::Down),
    expected: 7,
})]
#[case::page_up_moves_by_the_reported_rows(CursorRow {
    tracks: 20,
    from: 8,
    visible_rows: 5,
    message: BrowseRequest::PageBy(Nudge::Up),
    expected: 3,
})]
#[case::page_up_clamps_at_the_first_track(CursorRow {
    tracks: 20,
    from: 2,
    visible_rows: 5,
    message: BrowseRequest::PageBy(Nudge::Up),
    expected: 0,
})]
#[case::page_down_with_no_rows_reported_stays_put(CursorRow {
    tracks: 20,
    from: 0,
    visible_rows: 0,
    message: BrowseRequest::PageBy(Nudge::Down),
    expected: 0,
})]
#[case::page_down_on_an_empty_playlist_stays_at_zero(CursorRow {
    tracks: 0,
    from: 0,
    visible_rows: 5,
    message: BrowseRequest::PageBy(Nudge::Down),
    expected: 0,
})]
#[case::page_up_on_an_empty_playlist_stays_at_zero(CursorRow {
    tracks: 0,
    from: 0,
    visible_rows: 5,
    message: BrowseRequest::PageBy(Nudge::Up),
    expected: 0,
})]
fn cursor_row(#[case] row: CursorRow) {
    let mut model = browsing(row.tracks, row.from, &[]);
    model.workspace.visible_rows = row.visible_rows;
    let effects = browse(&mut model, row.message);
    assert_eq!(model.workspace.browse.selected().get(), row.expected);
    assert_eq!(effects, Cmd::None);
}

#[rstest]
#[case::no_rows_reported(0, 0)]
#[case::a_single_row(1, 1)]
#[case::a_full_page(20, 20)]
fn page_by_uses_the_stored_viewport(
    #[case] visible_rows: usize,
    #[case] expected: usize,
) {
    let mut model = browsing(30, 0, &[]);
    model.workspace.visible_rows = visible_rows;
    let _ = browse(&mut model, BrowseRequest::PageBy(Nudge::Down));
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
        library: Loaded::Ready(Library { all, view }),
        ..Default::default()
    }
}

#[rstest]
#[case::added_cycles_to_favorites(SortKey::Added, SortKey::Favorites)]
#[case::favorites_cycles_back_to_artist(SortKey::Favorites, SortKey::Artist)]
fn cycle_sort_walks_the_keys(#[case] from: SortKey, #[case] expected: SortKey) {
    let mut model = Model::default();
    model.workspace.browse.sort = from;
    let _ = browse(&mut model, BrowseRequest::CycleSort);
    assert_eq!(model.workspace.browse.sort, expected);
}

#[test]
fn cycle_sort_to_artist_reorders_the_view_and_the_playlist_under_it() {
    let mut model = unsorted_library();
    let _ = browse(&mut model, BrowseRequest::CycleSort);
    let _ = browse(&mut model, BrowseRequest::CycleSort);

    assert_eq!(model.workspace.browse.sort, SortKey::Artist);
    let sorted = vec![
        PathBuf::from("/music/a.flac"),
        PathBuf::from("/music/b.flac"),
        PathBuf::from("/music/c.flac"),
    ];
    let view: Vec<PathBuf> = model
        .library
        .view_tracks()
        .map(|(_, track)| track.path().to_path_buf())
        .collect();
    assert_eq!(view, sorted);
    assert_eq!(paths(&model.playlist.tracks), sorted);
    let all = model.library.ready().unwrap().all.len();
    assert_eq!(all, 3);
}

#[test]
fn cycle_sort_collapses_a_stale_shuffle_order_to_pending() {
    let mut model = unsorted_library();
    model.playlist.play_order = PlayOrder::Shuffle(vec![1, 0, 2]);
    let _ = browse(&mut model, BrowseRequest::CycleSort);
    assert_eq!(model.playlist.play_order, PlayOrder::ShufflePending);
}

#[test]
fn toggle_favorite_adds_then_removes_the_selected_track() {
    let mut model = browsing(2, 1, &[]);
    let selected = Path::new("/tmp/track1.flac");
    let other = Path::new("/tmp/track0.flac");

    let effects = browse(&mut model, BrowseRequest::ToggleFavorite);
    assert!(model.favorites.is_favorite(selected));
    assert!(!model.favorites.is_favorite(other));
    assert_eq!(effects, favorites_saved(&["/tmp/track1.flac"]));

    let untoggled = browse(&mut model, BrowseRequest::ToggleFavorite);
    assert!(!model.favorites.is_favorite(selected));
    assert_eq!(untoggled, favorites_saved(&[]));
}

#[test]
fn toggle_favorite_on_an_empty_playlist_writes_nothing() {
    let mut model = Model::default();
    let effects = browse(&mut model, BrowseRequest::ToggleFavorite);
    assert!(model.favorites.is_empty());
    assert_eq!(effects, Cmd::None);
}

#[test]
fn play_selected_jumps_the_playlist_and_starts_the_track() {
    let mut model = browsing(2, 1, &[]);
    let effects = browse(&mut model, BrowseRequest::PlaySelected);

    assert_eq!(model.playlist.anchor(), Some(PlaylistIndex::new(1)));
    assert!(matches!(
        &model.player,
        Player::Loading { track, .. } if track.path() == Path::new("/tmp/track1.flac")
    ));
    let track = bare_track(1);
    assert_eq!(
        effects,
        Cmd::Batch(vec![
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::Load {
                path: track.path().to_path_buf(),
                gain: None,
                revision: Revision::UNSTAMPED.next(),
            }),
            Effect::Library(LibraryCmd::AppendHistory {
                track: Arc::clone(&track),
                at: UnixSeconds::UNSTAMPED,
            }),
            Effect::System(SystemCmd::NowPlaying(NowPlaying::Track {
                title: track.song_title(),
                artist: None,
                album: None,
                duration: Duration::ZERO,
                path: track.path().to_path_buf(),
            })),
            Effect::Audio(AudioCmd::Pause(Playback::Playing)),
            Effect::System(SystemCmd::PlaybackState(Playback::Playing)),
            Effect::Animate(Cue::TrackChanged),
            Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Play)),
        ])
    );
}

#[test]
fn play_selected_retires_the_stream_the_media_key_started() {
    let mut model = browsing(3, 2, &[]);
    let _ = update(
        &mut model,
        Message::Playback(PlaybackRequest::Play),
        Moment::default(),
    )
    .unwrap();
    let _ = update(
        &mut model,
        Message::Audio(AudioEvent::Loaded { total: None }),
        Moment::default(),
    )
    .unwrap();

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
        .position(|effect| matches!(effect, Effect::Audio(AudioCmd::Load { .. })));
    assert!(
        matches!((retired, loaded), (Some(retired), Some(loaded)) if retired < loaded),
        "the media key's stream must be retired before the picked row loads, \
         or both sound at once, got {audio:?}"
    );
}

#[test]
fn play_selected_on_an_empty_playlist_starts_nothing() {
    let mut model = Model::default();
    let effects = browse(&mut model, BrowseRequest::PlaySelected);
    assert_eq!(model.playlist.anchor(), None);
    assert_eq!(model.player, Player::Stopped);
    assert_eq!(effects, Cmd::None);
}

#[test]
fn rescan_asks_once_until_the_scan_lands() {
    let mut model = Model {
        music_dir: PathBuf::from("/music"),
        ..Default::default()
    };

    let effects = browse(&mut model, BrowseRequest::Rescan);
    assert_eq!(
        effects,
        Cmd::One(Effect::Library(LibraryCmd::Rescan {
            root: PathBuf::from("/music"),
            revision: Revision::UNSTAMPED.next(),
        }))
    );
    assert_eq!(model.scan_status, ScanStatus::Scanning);

    let repeated = browse(&mut model, BrowseRequest::Rescan);
    assert_eq!(repeated, Cmd::None);
    assert_eq!(model.scan_status, ScanStatus::Scanning);
}

fn scanned(paths: &[&str]) -> Model {
    let all: Vec<_> = paths
        .iter()
        .map(|path| titled_track(path, path, ""))
        .collect();
    let view = (0..all.len()).map(TrackIndex::new).collect();
    let mut model = Model {
        library: Loaded::Ready(Library {
            all: all.clone(),
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
    let effects = browse(&mut model, BrowseRequest::Trash(PlaylistIndex::new(0)));

    let left = vec![PathBuf::from("/music/b.flac")];
    let all = paths(&model.library.ready().unwrap().all);
    assert_eq!(all, left);
    let view: Vec<PathBuf> = model
        .library
        .view_tracks()
        .map(|(_, track)| track.path().to_path_buf())
        .collect();
    assert_eq!(view, left);
    assert_eq!(paths(&model.playlist.tracks), left);
    assert_eq!(
        effects,
        Cmd::Batch(vec![
            Effect::Library(LibraryCmd::Trash(PathBuf::from("/music/a.flac"))),
            Effect::Animate(Cue::TrackDeleted),
        ])
    );
}

#[test]
fn trash_remaps_the_queue_and_drops_the_deleted_entry() {
    let mut model = scanned(&["/music/a.flac", "/music/b.flac", "/music/c.flac"]);
    model.queue = indices(&[2, 0]);

    let _ = browse(&mut model, BrowseRequest::Trash(PlaylistIndex::new(0)));

    assert_eq!(
        paths(&model.playlist.tracks),
        vec![
            PathBuf::from("/music/b.flac"),
            PathBuf::from("/music/c.flac"),
        ]
    );
    assert_eq!(model.queue, indices(&[1]));
}

#[test]
fn trash_on_an_empty_library_removes_nothing() {
    let mut model = scanned(&[]);
    let effects = browse(&mut model, BrowseRequest::Trash(PlaylistIndex::new(0)));
    assert!(model.library.ready().unwrap().all.is_empty());
    assert_eq!(effects, Cmd::None);
}
