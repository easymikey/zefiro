use kernel::{
    BrowseRequest,
    domain::{Cursor, CursorOver, Nudge, PlaylistIndex},
    update::overlay::{
        FollowUp,
        HistoryMessage,
        HistoryPick,
        HistoryRejection,
        OverlayEffect,
    },
};
use rstest::rstest;

use crate::support::table::{Cell, cell};

fn cursor(selected: usize, len: usize) -> CursorOver<()> {
    CursorOver {
        cursor: Cursor::with_len(len).at(selected),
        rows: (),
    }
}

fn nav(nudge: Nudge, len: usize) -> HistoryMessage {
    HistoryMessage::Navigate { nudge, len }
}

#[rstest]
#[case::nav_down_steps(cursor(0, 3), nav(Nudge::Down, 3), Ok((cursor(1, 3), OverlayEffect::default())))]
#[case::nav_up_clamps_at_the_top(cursor(0, 3), nav(Nudge::Up, 3), Ok((cursor(0, 3), OverlayEffect::default())))]
#[case::nav_down_clamps_at_the_bottom(cursor(2, 3), nav(Nudge::Down, 3), Ok((cursor(2, 3), OverlayEffect::default())))]
#[case::nav_resizes_to_a_shorter_log_first(cursor(5, 6), nav(Nudge::Down, 2), Ok((cursor(1, 2), OverlayEffect::default())))]
#[case::nav_on_an_empty_log_stays_put(cursor(0, 0), nav(Nudge::Down, 0), Ok((cursor(0, 0), OverlayEffect::default())))]
#[case::top_jumps_to_the_first_entry(cursor(2, 3), HistoryMessage::Top, Ok((cursor(0, 3), OverlayEffect::default())))]
#[case::bottom_resizes_then_jumps_to_the_last(
    cursor(0, 3),
    HistoryMessage::Bottom { len: 4 },
    Ok((cursor(3, 4), OverlayEffect::default()))
)]
#[case::enqueue_hands_the_router_the_resolved_track(
    cursor(1, 2),
    HistoryMessage::Enqueue(HistoryPick::Queued(PlaylistIndex::new(3))),
    Ok((
        cursor(1, 2),
        OverlayEffect::from(FollowUp::Browse(BrowseRequest::EnqueueTrack(PlaylistIndex::new(3))))
    ))
)]
#[case::enqueue_of_an_entry_that_left_the_library_is_refused(
    cursor(0, 1),
    HistoryMessage::Enqueue(HistoryPick::Missing),
    Err(HistoryRejection::NotInLibrary)
)]
#[case::enqueue_on_an_empty_log_is_refused(
    cursor(0, 0),
    HistoryMessage::Enqueue(HistoryPick::Nothing),
    Err(HistoryRejection::NothingSelected)
)]
fn history_cell(
    #[case] start: CursorOver<()>,
    #[case] message: HistoryMessage,
    #[case] expected: Cell<CursorOver<()>>,
) {
    cell(start, message, expected);
}
