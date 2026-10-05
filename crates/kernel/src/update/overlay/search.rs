use std::sync::Arc;

use crate::{
    cmd::Cmd,
    domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        index::ViewIndex,
        overlay::SearchQuery,
        track::Track,
    },
    message::{Message, QueueRequest, SearchEdit, SearchRequest},
    update::machine::{Machine, Unhandled},
};

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchRequest;
    type Effect = Cmd;

    fn transition(&mut self, message: SearchRequest) -> Result<Cmd, Unhandled> {
        match message {
            SearchRequest::Edit(edit) => {
                edit_query(&mut self.content.input, edit);
                Ok(Cmd::none())
            }
            SearchRequest::Navigate(direction) => {
                self.navigate(direction);
                Ok(Cmd::none())
            }
            SearchRequest::Enqueue => enqueue(self),
        }
    }
}

pub(crate) fn rank(search: &mut CursorOver<SearchQuery>, tracks: &[Arc<Track>]) {
    crate::search::rank_into(
        tracks,
        &search.content.input,
        &mut search.content.matches,
    );
    search.cursor = Cursor::new(search.content.matches.len());
}

fn edit_query(input: &mut String, edit: SearchEdit) {
    match edit {
        SearchEdit::Char(character) => input.push(character),
        SearchEdit::Backspace => {
            input.pop();
        }
        SearchEdit::DeleteWord => delete_trailing_word(input),
        SearchEdit::Clear => input.clear(),
    }
}

impl CursorOver<SearchQuery> {
    #[must_use]
    pub(crate) fn selected_match(&self) -> Option<ViewIndex> {
        self.content.matches.get(self.selected().get()).copied()
    }
}

fn enqueue(search: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search.selected_match().ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Queue(QueueRequest::EnqueueTrack(
        index,
    ))))
}

fn delete_trailing_word(input: &mut String) {
    let trimmed = input.trim_end();
    let cut = trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    input.truncate(cut);
}
