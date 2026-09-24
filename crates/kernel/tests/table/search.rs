use std::sync::Arc;

use kernel::{
    BrowseRequest,
    SearchEdit,
    domain::{Cursor, CursorOver, Nudge, PlaylistIndex, SearchQuery, Track},
    update::overlay::{FollowUp, OverlayEffect, SearchMessage, SearchRejection},
};
use rstest::rstest;

use crate::support::{
    table::{Cell, cell},
    titled_track,
};

fn query(input: &str, matches: Vec<usize>, selected: usize) -> CursorOver<SearchQuery> {
    let len = matches.len();
    CursorOver {
        cursor: Cursor::with_len(len).at(selected),
        rows: SearchQuery {
            input: input.to_string(),
            matches,
        },
    }
}

fn edit(edit: SearchEdit, tracks: Vec<Arc<Track>>) -> SearchMessage {
    SearchMessage::Edit(edit, tracks)
}

fn titled(titles: &[&str]) -> Vec<Arc<Track>> {
    titles
        .iter()
        .enumerate()
        .map(|(index, title)| titled_track(&format!("/tmp/{index}.flac"), title, ""))
        .collect()
}

#[rstest]
#[case::char_appends(query("m", vec![], 0), edit(SearchEdit::Char('o'), titled(&["mo", "zzz"])), Ok((query("mo", vec![0], 0), OverlayEffect::default())))]
#[case::char_on_an_empty_query_starts_it(query("", vec![0, 1], 0), edit(SearchEdit::Char('m'), titled(&["mo", "mars"])), Ok((query("m", vec![0, 1], 0), OverlayEffect::default())))]
#[case::backspace_erases(query("moo", vec![0], 0), edit(SearchEdit::Backspace, titled(&["mo", "zzz"])), Ok((query("mo", vec![0], 0), OverlayEffect::default())))]
#[case::backspace_on_an_empty_query_stays_empty(query("", vec![], 0), edit(SearchEdit::Backspace, vec![]), Ok((query("", vec![], 0), OverlayEffect::default())))]
#[case::delete_word_erases_the_trailing_word(query("foo bar", vec![], 0), edit(SearchEdit::DeleteWord, vec![]), Ok((query("foo ", vec![], 0), OverlayEffect::default())))]
#[case::delete_word_eats_the_trailing_space_with_the_word(query("foo bar ", vec![], 0), edit(SearchEdit::DeleteWord, vec![]), Ok((query("foo ", vec![], 0), OverlayEffect::default())))]
#[case::delete_word_on_one_word_empties_the_query(query("foo", vec![], 0), edit(SearchEdit::DeleteWord, vec![]), Ok((query("", vec![], 0), OverlayEffect::default())))]
#[case::clear_empties_the_query(query("foo bar", vec![], 0), edit(SearchEdit::Clear, vec![]), Ok((query("", vec![], 0), OverlayEffect::default())))]
#[case::editing_installs_fresh_matches_and_resets_the_cursor_to_the_top(
    query("m", vec![9, 9], 1),
    edit(SearchEdit::Char('o'), titled(&["mo", "zzz", "moon"])),
    Ok((query("mo", vec![0, 2], 0), OverlayEffect::default()))
)]
#[case::editing_to_nothing_leaves_nothing_selected(
    query("zz", vec![0, 1], 1),
    edit(SearchEdit::Char('z'), titled(&["mo", "moon"])),
    Ok((query("zzz", vec![], 0), OverlayEffect::default()))
)]
#[case::nav_down_steps(query("mo", vec![0, 2], 0), SearchMessage::Navigate(Nudge::Down), Ok((query("mo", vec![0, 2], 1), OverlayEffect::default())))]
#[case::nav_up_clamps_at_the_top(query("mo", vec![0, 2], 0), SearchMessage::Navigate(Nudge::Up), Ok((query("mo", vec![0, 2], 0), OverlayEffect::default())))]
#[case::nav_down_clamps_at_the_bottom(query("mo", vec![0, 2], 1), SearchMessage::Navigate(Nudge::Down), Ok((query("mo", vec![0, 2], 1), OverlayEffect::default())))]
#[case::enqueue_hands_the_router_the_selected_match(
    query("mo", vec![0, 2], 1),
    SearchMessage::Enqueue,
    Ok((
        query("mo", vec![0, 2], 1),
        OverlayEffect::from(FollowUp::Browse(BrowseRequest::EnqueueTrack(PlaylistIndex::new(2))))
    ))
)]
#[case::enqueue_without_a_match_is_refused(
    query("zzz", vec![], 0),
    SearchMessage::Enqueue,
    Err(SearchRejection::NothingSelected)
)]
fn search_cell(
    #[case] start: CursorOver<SearchQuery>,
    #[case] message: SearchMessage,
    #[case] expected: Cell<CursorOver<SearchQuery>>,
) {
    cell(start, message, expected);
}
