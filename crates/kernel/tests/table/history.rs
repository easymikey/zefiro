use kernel::{
    cmd::Cmd,
    domain::{cursor::Cursor, cursor_over::CursorOver},
    message::HistoryRequest,
    update::{machine::Unhandled, overlay::history::HistoryMessage},
};
use rstest::rstest;

use crate::support::table::cell;

fn cursor(selected_index: usize, len: usize) -> CursorOver<()> {
    CursorOver {
        cursor: Cursor::at(len, selected_index),
        content: (),
    }
}

#[rstest]
#[case::select_first_jumps_to_the_first_entry(cursor(2, 3), HistoryMessage { request: HistoryRequest::SelectFirst, rows: 3 }, Ok((cursor(0, 3), Cmd::none())))]
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
