use std::sync::Arc;

use crate::{
    domain::{Cursor, CursorOver, Direction, SearchQuery, Track, ViewIndex},
    message::{QueueRequest, SearchEdit},
    update::{
        machine::Machine,
        overlay::{FollowUp, OverlayOutcome},
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchMessage {
    Edit(SearchEdit, Vec<Arc<Track>>),
    Navigate(Direction),
    Enqueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SearchError {
    #[error("no search result selected")]
    NothingSelected,
}

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchMessage;
    type Error = SearchError;
    type Effect = OverlayOutcome;

    fn transition(
        &mut self,
        message: SearchMessage,
    ) -> Result<OverlayOutcome, SearchError> {
        match message {
            SearchMessage::Edit(edit, tracks) => {
                edit_query(&mut self.content.input, edit);
                crate::search::rank_into(
                    &tracks,
                    &self.content.input,
                    &mut self.content.matches,
                );
                self.cursor = Cursor::new(self.content.matches.len());
                Ok(OverlayOutcome::default())
            }
            SearchMessage::Navigate(direction) => {
                self.navigate(direction);
                Ok(OverlayOutcome::default())
            }
            SearchMessage::Enqueue => enqueue(self),
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

fn enqueue(search: &CursorOver<SearchQuery>) -> Result<OverlayOutcome, SearchError> {
    let index = search
        .content
        .matches
        .get(search.selected())
        .copied()
        .ok_or(SearchError::NothingSelected)?;
    let queued = FollowUp::Queue(QueueRequest::EnqueueTrack(ViewIndex::new(index)));
    Ok(OverlayOutcome::from(queued))
}

fn delete_trailing_word(input: &mut String) {
    let trimmed = input.trim_end();
    let cut = trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    input.truncate(cut);
}
