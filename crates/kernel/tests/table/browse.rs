use std::sync::Arc;

use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        cue::Cue,
        cursor::Cursor,
        direction::Direction,
        geometry::Cells,
        index::{TrackIndex, ViewIndex},
        library::Library,
        model::Model,
        playlist::PlayOrder,
        time::Moment,
        track::Track,
    },
    message::{BrowseRequest, Message, QueueRequest},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    model_with_tracks,
    titled_track,
    update::{send, update},
};

fn queue(model: &mut Model, request: QueueRequest) -> Result<Cmd, Unhandled> {
    update(model, Message::Queue(request), Moment::default())
}

fn queued_tracks(model: &Model, rows: &[usize]) -> Vec<Arc<Track>> {
    rows.iter()
        .map(|&row| Arc::clone(&model.playlist.tracks[row]))
        .collect()
}

fn browsing(count: usize, selected_index: usize, rows: &[usize]) -> Model {
    let mut model = model_with_tracks(count);
    model.workspace.browse.cursor = Cursor::at(count, selected_index);
    model.queue = queued_tracks(&model, rows);
    model
}

struct QueueRow {
    model: Model,
    request: QueueRequest,
    queued: &'static [usize],
    effects: Result<Cmd, Unhandled>,
}

fn queue_changed() -> Result<Cmd, Unhandled> {
    Ok(Cmd::effect(Effect::Animate(Cue::QueueChanged)))
}

#[rstest]
#[case::enqueue_appends_to_the_back(QueueRow {
    model: browsing(2, 1, &[0]),
    request: QueueRequest::Toggle,
    queued: &[0, 1],
    effects: queue_changed(),
})]
#[case::play_next_inserts_at_the_front(QueueRow {
    model: browsing(2, 1, &[0]),
    request: QueueRequest::PlayNext,
    queued: &[1, 0],
    effects: queue_changed(),
})]
#[case::dequeue_removes_the_single_occurrence(QueueRow {
    model: browsing(2, 1, &[1, 0]),
    request: QueueRequest::Dequeue,
    queued: &[0],
    effects: queue_changed(),
})]
#[case::move_down_swaps_with_the_successor(QueueRow {
    model: browsing(3, 0, &[0, 1, 2]),
    request: QueueRequest::Move(Direction::Next),
    queued: &[1, 0, 2],
    effects: queue_changed(),
})]
fn queue_row(#[case] row: QueueRow) {
    let QueueRow {
        mut model,
        request,
        queued,
        effects,
    } = row;
    let seen = queue(&mut model, request);
    assert_eq!(model.queue, queued_tracks(&model, queued));
    assert_eq!(seen, effects);
}

#[rstest]
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
        library: Some(Library {
            tracks: all,
            track_indexes: view,
        }),
        ..Default::default()
    }
}

#[test]
fn cycle_sort_collapses_a_stale_shuffle_order_to_pending() {
    let mut model = unsorted_library();
    model.playlist.play_order =
        PlayOrder::Shuffled([1, 0, 2].map(ViewIndex::new).to_vec());
    send(&mut model, Message::Browse(BrowseRequest::CycleSort));
    assert_eq!(model.playlist.play_order, PlayOrder::ShufflePending);
}
