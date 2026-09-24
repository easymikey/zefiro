use std::sync::Arc;

use crate::{
    domain::{
        Cursor,
        CursorOver,
        ListMotion,
        Nudge,
        PlaylistIndex,
        SearchQuery,
        Track,
    },
    message::{BrowseRequest, SearchEdit},
    update::{
        machine::{Machine, Rejected},
        overlay::{FollowUp, OverlayEffect},
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchMessage {
    Edit(SearchEdit, Vec<Arc<Track>>),
    Navigate(Nudge),
    Enqueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRejection {
    NothingSelected,
}

type Transition =
    Result<(CursorOver<SearchQuery>, OverlayEffect), Rejected<CursorOver<SearchQuery>>>;

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchMessage;
    type Rejection = SearchRejection;
    type Effect = OverlayEffect;

    fn transition(mut self, message: SearchMessage) -> Transition {
        match message {
            SearchMessage::Edit(edit, tracks) => {
                edit_query(&mut self.rows.input, edit);
                let mut matches = std::mem::take(&mut self.rows.matches);
                crate::search::rank_into(&tracks, &self.rows.input, &mut matches);
                self.cursor = Cursor::new(matches.len());
                self.rows.matches = matches;
                Ok((self, OverlayEffect::default()))
            }
            SearchMessage::Navigate(nudge) => {
                self.navigate(ListMotion::from(nudge));
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
    match search.rows.matches.get(search.selected()).copied() {
        Some(index) => {
            let queued = FollowUp::Browse(BrowseRequest::EnqueueTrack(
                PlaylistIndex::new(index),
            ));
            Ok((search, OverlayEffect::from(queued)))
        }
        None => Err(Rejected {
            state: search,
            reason: SearchRejection::NothingSelected,
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
