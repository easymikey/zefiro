use std::path::Path;

use crate::{
    Cmd,
    domain::{
        CursorOver,
        Direction,
        History,
        Model,
        Moment,
        Overlay,
        PlaylistIndex,
        Toast,
        Workspace,
        playlist::Playlist,
    },
    message::{HistoryRequest, QueueRequest, WorkspaceRequest},
    update::{
        error::UpdateError,
        machine::{Machine, Rejected},
        overlay::{
            FollowUp,
            InnerMessage,
            OverlayEffect,
            OverlayError,
            OverlayMessage,
            follow,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPick {
    Queued(PlaylistIndex),
    Missing,
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryMessage {
    Navigate { direction: Direction, len: usize },
    Top,
    Bottom { len: usize },
    Enqueue(HistoryPick),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryError {
    NothingSelected,
    NotInLibrary,
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Error = HistoryError;
    type Effect = OverlayEffect;

    fn transition(
        mut self,
        message: HistoryMessage,
    ) -> Result<(Self, OverlayEffect), Rejected<Self>> {
        match message {
            HistoryMessage::Navigate { direction, len } => {
                self.resize(len);
                self.navigate(direction);
                Ok((self, OverlayEffect::default()))
            }
            HistoryMessage::Top => {
                self.cursor = self.cursor.first();
                Ok((self, OverlayEffect::default()))
            }
            HistoryMessage::Bottom { len } => {
                self.resize(len);
                self.cursor = self.cursor.last();
                Ok((self, OverlayEffect::default()))
            }
            HistoryMessage::Enqueue(HistoryPick::Queued(index)) => {
                let queued = FollowUp::Queue(QueueRequest::EnqueueTrack(index));
                Ok((self, OverlayEffect::from(queued)))
            }
            HistoryMessage::Enqueue(HistoryPick::Missing) => Err(Rejected {
                state: self,
                reason: HistoryError::NotInLibrary,
            }),
            HistoryMessage::Enqueue(HistoryPick::Nothing) => Err(Rejected {
                state: self,
                reason: HistoryError::NothingSelected,
            }),
        }
    }
}

pub(crate) fn request(
    model: &mut Model,
    request: HistoryRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let len = model.history.view.len();
    let message = match request {
        HistoryRequest::Navigate(direction) => {
            HistoryMessage::Navigate { direction, len }
        }
        HistoryRequest::Top => HistoryMessage::Top,
        HistoryRequest::Bottom => HistoryMessage::Bottom { len },
        HistoryRequest::Enqueue => HistoryMessage::Enqueue(pick(
            &model.workspace,
            &model.history,
            &model.playlist,
        )),
    };
    match model
        .workspace
        .overlay
        .update(OverlayMessage::Inner(InnerMessage::History(message)))
    {
        Ok(effect) => follow(model, effect, now),
        Err(OverlayError::History(HistoryError::NotInLibrary)) => {
            not_in_library(&mut model.workspace)
        }
        Err(
            rejection @ (OverlayError::WhileClosed
            | OverlayError::NoTrack
            | OverlayError::WrongOverlay
            | OverlayError::NoConfirm
            | OverlayError::NothingSelected
            | OverlayError::Jump(_)
            | OverlayError::Search(_)
            | OverlayError::History(HistoryError::NothingSelected)),
        ) => Err(rejection.into()),
    }
}

fn pick(workspace: &Workspace, history: &History, playlist: &Playlist) -> HistoryPick {
    selected_path(workspace, history).map_or(HistoryPick::Nothing, |path| {
        playlist
            .tracks
            .iter()
            .position(|track| track.path() == path)
            .map_or(HistoryPick::Missing, |index| {
                HistoryPick::Queued(PlaylistIndex::new(index))
            })
    })
}

fn selected_path<'a>(workspace: &Workspace, history: &'a History) -> Option<&'a Path> {
    match &workspace.overlay {
        Some(Overlay::History(cursor)) => history
            .view
            .get(cursor.selected())
            .map(|entry| entry.path.as_path()),
        Some(
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::SavePlaylist { .. }
            | Overlay::Settings { .. }
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::MusicDir { .. },
        )
        | None => None,
    }
}

fn not_in_library(workspace: &mut Workspace) -> Result<Cmd, UpdateError> {
    Ok(workspace.update(WorkspaceRequest::ShowToast(Toast::info(
        "Not in library".to_string(),
    )))?)
}
