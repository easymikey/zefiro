use std::path::Path;

use crate::{
    Cmd,
    domain::{
        CursorOver,
        History,
        ListMotion,
        Model,
        Moment,
        Nudge,
        Overlay,
        PlaylistIndex,
        Toast,
        Workspace,
        playlist::Playlist,
    },
    message::{BrowseRequest, HistoryRequest, WorkspaceRequest},
    update::{
        machine::{Machine, Rejected},
        overlay::{
            FollowUp,
            InnerMessage,
            OverlayEffect,
            OverlayMessage,
            OverlayRejection,
            follow,
        },
        rejection::Rejection,
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
    Navigate { nudge: Nudge, len: usize },
    Top,
    Bottom { len: usize },
    Enqueue(HistoryPick),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRejection {
    NothingSelected,
    NotInLibrary,
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Rejection = HistoryRejection;
    type Effect = OverlayEffect;

    fn transition(
        mut self,
        message: HistoryMessage,
    ) -> Result<(Self, OverlayEffect), Rejected<Self>> {
        match message {
            HistoryMessage::Navigate { nudge, len } => {
                self.resize(len);
                self.navigate(ListMotion::from(nudge));
                Ok((self, OverlayEffect::default()))
            }
            HistoryMessage::Top => {
                self.navigate(ListMotion::First);
                Ok((self, OverlayEffect::default()))
            }
            HistoryMessage::Bottom { len } => {
                self.resize(len);
                self.navigate(ListMotion::Last);
                Ok((self, OverlayEffect::default()))
            }
            HistoryMessage::Enqueue(HistoryPick::Queued(index)) => {
                let queued = FollowUp::Browse(BrowseRequest::EnqueueTrack(index));
                Ok((self, OverlayEffect::from(queued)))
            }
            HistoryMessage::Enqueue(HistoryPick::Missing) => Err(Rejected {
                state: self,
                reason: HistoryRejection::NotInLibrary,
            }),
            HistoryMessage::Enqueue(HistoryPick::Nothing) => Err(Rejected {
                state: self,
                reason: HistoryRejection::NothingSelected,
            }),
        }
    }
}

pub(super) fn request(
    model: &mut Model,
    request: HistoryRequest,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let len = model.history.view.len();
    let message = match request {
        HistoryRequest::Navigate(nudge) => HistoryMessage::Navigate { nudge, len },
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
        Err(OverlayRejection::History(HistoryRejection::NotInLibrary)) => {
            not_in_library(&mut model.workspace)
        }
        Err(
            rejection @ (OverlayRejection::WhileClosed
            | OverlayRejection::NoTrack
            | OverlayRejection::WrongOverlay
            | OverlayRejection::NoConfirm
            | OverlayRejection::NothingSelected
            | OverlayRejection::Jump(_)
            | OverlayRejection::Search(_)
            | OverlayRejection::History(HistoryRejection::NothingSelected)),
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
            | Overlay::Settings(_)
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::SourceDir { .. },
        )
        | None => None,
    }
}

fn not_in_library(workspace: &mut Workspace) -> Result<Cmd, Rejection> {
    Ok(workspace.update(WorkspaceRequest::ShowToast(Toast::info(
        "Not in library".to_string(),
    )))?)
}
