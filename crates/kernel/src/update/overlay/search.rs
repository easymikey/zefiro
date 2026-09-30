use std::sync::Arc;

use crate::{
    domain::{Cursor, CursorOver, Direction, PlaylistIndex, SearchQuery, Track},
    message::{QueueRequest, SearchEdit},
    update::{
        machine::{Machine, Rejected},
        overlay::{FollowUp, OverlayEffect},
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchMessage {
    Edit(SearchEdit, Vec<Arc<Track>>),
    Navigate(Direction),
    Enqueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    NothingSelected,
}

type Transition =
    Result<(CursorOver<SearchQuery>, OverlayEffect), Rejected<CursorOver<SearchQuery>>>;

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchMessage;
    type Error = SearchError;
    type Effect = OverlayEffect;

    fn transition(mut self, message: SearchMessage) -> Transition {
        match message {
            SearchMessage::Edit(edit, tracks) => {
                edit_query(&mut self.content.input, edit);
                let mut matches = std::mem::take(&mut self.content.matches);
                crate::search::rank_into(&tracks, &self.content.input, &mut matches);
                self.cursor = Cursor::new(matches.len());
                self.content.matches = matches;
                Ok((self, OverlayEffect::default()))
            }
            SearchMessage::Navigate(direction) => {
                self.navigate(direction);
                Ok((self, OverlayEffect::default()))
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

fn enqueue(search: CursorOver<SearchQuery>) -> Transition {
    match search.content.matches.get(search.selected()).copied() {
        Some(index) => {
            let queued =
                FollowUp::Queue(QueueRequest::EnqueueTrack(PlaylistIndex::new(index)));
            Ok((search, OverlayEffect::from(queued)))
        }
        None => Err(Rejected {
            state: search,
            reason: SearchError::NothingSelected,
        }),
    }
}

fn delete_trailing_word(input: &mut String) {
    let trimmed = input.trim_end();
    let cut = trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    input.truncate(cut);
}
