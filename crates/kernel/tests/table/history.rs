use kernel::{
    QueueRequest,
    domain::{Cursor, CursorOver, Direction, ViewIndex},
    update::overlay::{
        FollowUp,
        HistoryError,
        HistoryMessage,
        HistoryPick,
        OverlayOutcome,
    },
};
use rstest::rstest;

use crate::support::table::{Cell, cell};

fn cursor(selected: usize, len: usize) -> CursorOver<()> {
    CursorOver {
        cursor: Cursor::with_len(len).at(selected),
        content: (),
    }
}

fn nav(direction: Direction, len: usize) -> HistoryMessage {
    HistoryMessage::Navigate { direction, len }
}

#[rstest]
#[case::nav_down_steps(cursor(0, 3), nav(Direction::Next, 3), Ok((cursor(1, 3), OverlayOutcome::default())))]
#[case::nav_up_clamps_at_the_top(cursor(0, 3), nav(Direction::Previous, 3), Ok((cursor(0, 3), OverlayOutcome::default())))]
#[case::nav_down_clamps_at_the_bottom(cursor(2, 3), nav(Direction::Next, 3), Ok((cursor(2, 3), OverlayOutcome::default())))]
#[case::nav_resizes_to_a_shorter_log_first(cursor(5, 6), nav(Direction::Next, 2), Ok((cursor(1, 2), OverlayOutcome::default())))]
#[case::nav_on_an_empty_log_stays_put(cursor(0, 0), nav(Direction::Next, 0), Ok((cursor(0, 0), OverlayOutcome::default())))]
#[case::top_jumps_to_the_first_entry(cursor(2, 3), HistoryMessage::Top, Ok((cursor(0, 3), OverlayOutcome::default())))]
#[case::bottom_resizes_then_jumps_to_the_last(
    cursor(0, 3),
    HistoryMessage::Bottom { len: 4 },
    Ok((cursor(3, 4), OverlayOutcome::default()))
)]
#[case::enqueue_hands_the_router_the_resolved_track(
    cursor(1, 2),
    HistoryMessage::Enqueue(HistoryPick::Queued(ViewIndex::new(3))),
    Ok((
        cursor(1, 2),
        OverlayOutcome::from(FollowUp::Queue(QueueRequest::EnqueueTrack(ViewIndex::new(3))))
    ))
)]
#[case::enqueue_of_an_entry_that_left_the_library_is_refused(
    cursor(0, 1),
    HistoryMessage::Enqueue(HistoryPick::Missing),
    Err(HistoryError::NotInLibrary)
)]
#[case::enqueue_on_an_empty_log_is_refused(
    cursor(0, 0),
    HistoryMessage::Enqueue(HistoryPick::Nothing),
    Err(HistoryError::NothingSelected)
)]
fn history_cell(
    #[case] start: CursorOver<()>,
    #[case] message: HistoryMessage,
    #[case] expected: Cell<CursorOver<()>>,
) {
    cell(start, message, expected);
}
