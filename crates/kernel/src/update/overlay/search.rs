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
    update::machine::{Machine, Unhandled, replace},
};

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchRequest;
    type Effect = Cmd;

    fn transition(&mut self, message: SearchRequest) -> Result<Cmd, Unhandled> {
        match message {
            SearchRequest::Edit(edit) => {
                edit_query(&mut self.content.input, edit)?;
                Ok(Cmd::none())
            }
            SearchRequest::Navigate(direction) => {
                let moved = self.cursor.step(direction.sign());
                replace(&mut self.cursor, moved).map(|()| Cmd::none())
            }
            SearchRequest::Enqueue => enqueue(self),
        }
    }
}

pub(crate) fn rerank(
    search_query: &mut CursorOver<SearchQuery>,
    tracks: &[Arc<Track>],
) {
    let ranked = crate::search::rank(tracks, &search_query.content.input);
    refreshed(search_query, ranked);
}

pub(crate) fn requery(
    search_query: &mut CursorOver<SearchQuery>,
    tracks: &[Arc<Track>],
    edit: SearchEdit,
) {
    let input = &search_query.content.input;
    let ranked = match edit {
        SearchEdit::Char(_) => {
            crate::search::narrow(tracks, input, &search_query.content.matches)
        }
        SearchEdit::Backspace | SearchEdit::DeleteWord | SearchEdit::Clear => {
            crate::search::rank(tracks, input)
        }
    };
    refreshed(search_query, ranked);
}

fn refreshed(search_query: &mut CursorOver<SearchQuery>, matches: Vec<ViewIndex>) {
    search_query.cursor = Cursor::new(matches.len());
    search_query.content.matches = matches;
}

fn edit_query(input: &mut String, edit: SearchEdit) -> Result<(), Unhandled> {
    match edit {
        SearchEdit::Backspace | SearchEdit::DeleteWord | SearchEdit::Clear
            if input.is_empty() =>
        {
            return Err(Unhandled);
        }
        SearchEdit::Char(character) => input.push(character),
        SearchEdit::Backspace => {
            input.pop();
        }
        SearchEdit::DeleteWord => delete_trailing_word(input),
        SearchEdit::Clear => input.clear(),
    }
    Ok(())
}

impl CursorOver<SearchQuery> {
    #[must_use]
    pub(crate) fn selected_match(&self) -> Option<ViewIndex> {
        self.content.matches.get(self.selected().get()).copied()
    }
}

fn enqueue(search_query: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search_query.selected_match().ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Queue(QueueRequest::ToggleAt(index))))
}

fn delete_trailing_word(input: &mut String) {
    let trimmed = input.trim_end();
    let cut = trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    input.truncate(cut);
}
