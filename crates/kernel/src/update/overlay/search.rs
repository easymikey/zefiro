use std::sync::Arc;

use crate::{
    Cmd,
    domain::{Cursor, CursorOver, Direction, SearchQuery, Track},
    message::{Message, QueueRequest, SearchEdit},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchQueryMessage {
    Edit(SearchEdit, Vec<Arc<Track>>),
    Navigate(Direction),
    Enqueue,
}

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchQueryMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: SearchQueryMessage) -> Result<Cmd, Unhandled> {
        match message {
            SearchQueryMessage::Edit(edit, tracks) => {
                edit_query(&mut self.content.input, edit);
                crate::search::rank_into(
                    &tracks,
                    &self.content.input,
                    &mut self.content.matches,
                );
                self.cursor = Cursor::new(self.content.matches.len());
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
