use kernel::{
    cmd::Cmd,
    domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        index::ViewIndex,
        overlay::SearchQuery,
    },
    message::{Message, QueueRequest, SearchEdit, SearchRequest},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::table::cell;

fn query(input: &str, matches: Vec<usize>, selected: usize) -> CursorOver<SearchQuery> {
    let len = matches.len();
    CursorOver {
        cursor: Cursor::at(len, selected),
        content: SearchQuery {
            input: input.to_string(),
            matches: matches.into_iter().map(ViewIndex::new).collect(),
        },
    }
}

fn edit(edit: SearchEdit) -> SearchRequest {
    SearchRequest::Edit(edit)
}

#[rstest]
#[case::char_appends(query("m", vec![], 0), edit(SearchEdit::Char('o')), Ok((query("mo", vec![], 0), Cmd::none())))]
#[case::char_on_an_empty_query_starts_it(query("", vec![0, 1], 0), edit(SearchEdit::Char('m')), Ok((query("m", vec![0, 1], 0), Cmd::none())))]
#[case::backspace_erases(query("moo", vec![0], 0), edit(SearchEdit::Backspace), Ok((query("mo", vec![0], 0), Cmd::none())))]
#[case::backspace_on_an_empty_query_stays_empty(query("", vec![], 0), edit(SearchEdit::Backspace), Ok((query("", vec![], 0), Cmd::none())))]
#[case::delete_word_erases_the_trailing_word(query("foo bar", vec![], 0), edit(SearchEdit::DeleteWord), Ok((query("foo ", vec![], 0), Cmd::none())))]
#[case::delete_word_eats_the_trailing_space_with_the_word(query("foo bar ", vec![], 0), edit(SearchEdit::DeleteWord), Ok((query("foo ", vec![], 0), Cmd::none())))]
#[case::delete_word_on_one_word_empties_the_query(query("foo", vec![], 0), edit(SearchEdit::DeleteWord), Ok((query("", vec![], 0), Cmd::none())))]
#[case::clear_empties_the_query(query("foo bar", vec![], 0), edit(SearchEdit::Clear), Ok((query("", vec![], 0), Cmd::none())))]
#[case::nav_down_steps(query("mo", vec![0, 2], 0), SearchRequest::Navigate(Direction::Next), Ok((query("mo", vec![0, 2], 1), Cmd::none())))]
#[case::nav_up_clamps_at_the_top(query("mo", vec![0, 2], 0), SearchRequest::Navigate(Direction::Previous), Ok((query("mo", vec![0, 2], 0), Cmd::none())))]
#[case::nav_down_clamps_at_the_bottom(query("mo", vec![0, 2], 1), SearchRequest::Navigate(Direction::Next), Ok((query("mo", vec![0, 2], 1), Cmd::none())))]
#[case::enqueue_hands_the_router_the_selected_match(
    query("mo", vec![0, 2], 1),
    SearchRequest::Enqueue,
    Ok((
        query("mo", vec![0, 2], 1),
        Cmd::message(Message::Queue(QueueRequest::EnqueueTrack(ViewIndex::new(2))))
    ))
)]
#[case::enqueue_without_a_match_is_refused(
    query("zzz", vec![], 0),
    SearchRequest::Enqueue,
    Err(Unhandled)
)]
fn search_cell(
    #[case] start: CursorOver<SearchQuery>,
    #[case] message: SearchRequest,
    #[case] expected: Result<
        (
            CursorOver<SearchQuery>,
            <CursorOver<SearchQuery> as kernel::update::machine::Machine>::Effect,
        ),
        Unhandled,
    >,
) {
    cell(start, message, expected);
}
