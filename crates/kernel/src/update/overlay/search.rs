use std::sync::Arc;

use crate::{
    Cmd,
    domain::{Cursor, CursorOver, Direction, SearchQuery, Track},
    message::{Message, QueueRequest, SearchEdit},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchQueryMessage {
    Edit(SearchEdit),
    Navigate(Direction),
    Enqueue,
}

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchQueryMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: SearchQueryMessage) -> Result<Cmd, Unhandled> {
        match message {
            SearchQueryMessage::Edit(edit) => {
                edit_query(&mut self.content.input, edit);
                Ok(Cmd::none())
            }
            SearchQueryMessage::Navigate(direction) => {
                self.navigate(direction);
                Ok(Cmd::none())
            }
            SearchQueryMessage::Enqueue => enqueue(self),
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

fn enqueue(search: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search
        .content
        .matches
        .get(search.selected().get())
        .copied()
        .ok_or(Unhandled)?;
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
