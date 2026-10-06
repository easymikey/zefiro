use kernel::{
    cmd::Cmd,
    domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        time::Moment,
        track::TrackSource,
    },
    message::{Message, QueueRequest},
    update::{machine::Unhandled, overlay::history::HistoryMessage},
};
use rstest::rstest;

use crate::support::{
    router::{history_enqueue, logged},
    table::cell,
    update::{send, update},
};

fn cursor(selected_index: usize, len: usize) -> CursorOver<()> {
    CursorOver {
        cursor: Cursor::at(len, selected_index),
        content: (),
    }
}

fn nav(direction: Direction, len: usize) -> HistoryMessage {
    HistoryMessage::Navigate { direction, len }
}

#[rstest]
#[case::nav_down_steps(cursor(0, 3), nav(Direction::Next, 3), Ok((cursor(1, 3), Cmd::none())))]
#[case::nav_up_at_the_top_is_refused(
    cursor(0, 3),
    nav(Direction::Previous, 3),
    Err(Unhandled)
)]
#[case::nav_down_at_the_bottom_is_refused(
    cursor(2, 3),
    nav(Direction::Next, 3),
    Err(Unhandled)
)]
#[case::nav_resizes_to_a_shorter_log_first(cursor(5, 6), nav(Direction::Next, 2), Ok((cursor(1, 2), Cmd::none())))]
#[case::nav_on_an_empty_log_is_refused(
    cursor(0, 0),
    nav(Direction::Next, 0),
    Err(Unhandled)
)]
#[case::select_first_jumps_to_the_first_entry(cursor(2, 3), HistoryMessage::SelectFirst, Ok((cursor(0, 3), Cmd::none())))]
#[case::select_first_on_the_first_entry_is_refused(
    cursor(0, 3),
    HistoryMessage::SelectFirst,
    Err(Unhandled)
)]
#[case::select_last_on_the_last_entry_is_refused(
    cursor(2, 3),
    HistoryMessage::SelectLast { rows: 3 },
    Err(Unhandled)
)]
#[case::select_last_resizes_then_jumps_to_the_last(
    cursor(0, 3),
    HistoryMessage::SelectLast { rows: 4 },
    Ok((cursor(3, 4), Cmd::none()))
)]
#[case::enqueue_names_the_entry_under_the_cursor(
    cursor(1, 2),
    HistoryMessage::Enqueue(2),
    Ok((
        cursor(1, 2),
        Cmd::message(Message::Queue(QueueRequest::ToggleHistoryEntry(1)))
    ))
)]
#[case::enqueue_names_the_cursor_not_the_first_entry(
    cursor(2, 3),
    HistoryMessage::Enqueue(3),
    Ok((
        cursor(2, 3),
        Cmd::message(Message::Queue(QueueRequest::ToggleHistoryEntry(2)))
    ))
)]
#[case::enqueue_with_the_cursor_past_the_end_is_refused(
    cursor(1, 2),
    HistoryMessage::Enqueue(1),
    Err(Unhandled)
)]
#[case::enqueue_on_an_empty_log_is_refused(
    cursor(0, 0),
    HistoryMessage::Enqueue(0),
    Err(Unhandled)
)]
fn history_cell(
    #[case] start: CursorOver<()>,
    #[case] message: HistoryMessage,
    #[case] expected: Result<
        (
            CursorOver<()>,
            <CursorOver<()> as kernel::update::machine::Machine>::Effect,
        ),
        Unhandled,
    >,
) {
    cell(start, message, expected);
}

#[test]
fn history_enter_on_a_queued_entry_enqueues_its_library_track() {
    let mut model = logged(&["/m/a.flac", "/m/b.flac"], &["/m/a.flac", "/m/b.flac"]);
    send(&mut model, history_enqueue());

    assert_eq!(model.queue, vec![TrackSource::Local("/m/b.flac".into())]);
    assert!(model.workspace.toasts.is_empty());
}

#[test]
fn history_enter_on_an_entry_missing_from_the_library_raises_the_toast() {
    let mut model = logged(&["/m/gone.flac"], &[]);
    send(&mut model, history_enqueue());

    let titles: Vec<&str> = model
        .workspace
        .toasts
        .iter()
        .map(|toast| toast.title.as_str())
        .collect();
    assert_eq!(titles, vec!["Not in library"]);
    assert!(model.queue.is_empty());
}

#[test]
fn history_enter_past_the_end_of_the_log_is_refused() {
    let mut model = logged(&[], &["/m/a.flac"]);

    assert_eq!(
        update(&mut model, history_enqueue(), Moment::default()),
        Err(Unhandled)
    );
}
