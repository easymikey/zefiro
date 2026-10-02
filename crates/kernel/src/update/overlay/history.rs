use std::path::Path;

use crate::{
    Cmd,
    domain::{
        CursorOver,
        Direction,
        HistoryEntry,
        Model,
        Moment,
        Overlay,
        Toast,
        ViewIndex,
        Workspace,
        playlist::Playlist,
    },
    message::{HistoryRequest, QueueRequest},
    update::{
        error::UpdateError,
        machine::Machine,
        overlay::{
            FollowUp,
            InnerMessage,
            OverlayError,
            OverlayMessage,
            OverlayOutcome,
            follow,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPick {
    Queued(ViewIndex),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    #[error("no history entry selected")]
    NothingSelected,
    #[error("track is not in the library")]
    NotInLibrary,
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Error = HistoryError;
    type Effect = OverlayOutcome;

    fn transition(
        &mut self,
        message: HistoryMessage,
    ) -> Result<OverlayOutcome, HistoryError> {
        match message {
            HistoryMessage::Navigate { direction, len } => {
                self.resize(len);
                self.navigate(direction);
                Ok(OverlayOutcome::default())
            }
            HistoryMessage::Top => {
                self.cursor = self.cursor.first();
                Ok(OverlayOutcome::default())
            }
            HistoryMessage::Bottom { len } => {
                self.resize(len);
                self.cursor = self.cursor.last();
                Ok(OverlayOutcome::default())
            }
            HistoryMessage::Enqueue(HistoryPick::Queued(index)) => {
                let queued = FollowUp::Queue(QueueRequest::EnqueueTrack(index));
                Ok(OverlayOutcome::from(queued))
            }
            HistoryMessage::Enqueue(HistoryPick::Missing) => {
                Err(HistoryError::NotInLibrary)
            }
            HistoryMessage::Enqueue(HistoryPick::Nothing) => {
                Err(HistoryError::NothingSelected)
            }
        }
    }
}

pub(crate) fn request(
    model: &mut Model,
    request: HistoryRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let len = model.history.len();
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
        .transition(OverlayMessage::Inner(InnerMessage::History(message)))
    {
        Ok(effect) => follow(model, effect, now),
        Err(OverlayError::History(HistoryError::NotInLibrary)) => not_in_library(model),
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

fn pick(
    workspace: &Workspace,
    history: &[HistoryEntry],
    playlist: &Playlist,
) -> HistoryPick {
    selected_path(workspace, history).map_or(HistoryPick::Nothing, |path| {
        playlist
            .tracks
            .iter()
            .position(|track| track.path() == path)
            .map_or(HistoryPick::Missing, |index| {
                HistoryPick::Queued(ViewIndex::new(index))
            })
    })
}

fn selected_path<'a>(
    workspace: &Workspace,
    history: &'a [HistoryEntry],
) -> Option<&'a Path> {
    match &workspace.overlay {
        Some(Overlay::History(cursor)) => history
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

fn not_in_library(model: &mut Model) -> Result<Cmd, UpdateError> {
    Ok(model.workspace.show(
        Toast::info("Not in library".to_string()),
        &mut model.revisions,
    ))
}
