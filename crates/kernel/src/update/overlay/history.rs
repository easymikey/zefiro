use crate::{
    cmd::Cmd,
    domain::{
        cursor_over::CursorOver,
        direction::Direction,
        history::HistoryEntry,
        index::ViewIndex,
        overlay::Overlay,
        playlist::Playlist,
        toast::Toast,
        track::TrackRef,
        workspace::Workspace,
    },
    message::{HistoryRequest, Message, QueueRequest},
    update::{
        machine::{Machine, Unhandled},
        overlay::{OverlayContentMessage, OverlayMessage, OverlayParts},
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
    Bottom(usize),
    Enqueue(HistoryPick),
}

impl Machine for CursorOver<()> {
    type Message = HistoryMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: HistoryMessage) -> Result<Cmd, Unhandled> {
        match message {
            HistoryMessage::Navigate { direction, len } => {
                self.resize(len);
                self.navigate(direction);
                Ok(Cmd::none())
            }
            HistoryMessage::Top => {
                self.cursor = self.cursor.first();
                Ok(Cmd::none())
            }
            HistoryMessage::Bottom(len) => {
                self.resize(len);
                self.cursor = self.cursor.last();
                Ok(Cmd::none())
            }
            HistoryMessage::Enqueue(HistoryPick::Queued(index)) => Ok(Cmd::message(
                Message::Queue(QueueRequest::EnqueueTrack(index)),
            )),
            HistoryMessage::Enqueue(HistoryPick::Missing) => Ok(Cmd::message(
                Message::Toast(Toast::info("Not in library".to_string())),
            )),
            HistoryMessage::Enqueue(HistoryPick::Nothing) => Err(Unhandled),
        }
    }
}

pub(crate) fn request(
    parts: &mut OverlayParts<'_>,
    request: HistoryRequest,
) -> Result<Cmd, Unhandled> {
    let len = parts.history.len();
    let message = match request {
        HistoryRequest::Navigate(direction) => {
            HistoryMessage::Navigate { direction, len }
        }
        HistoryRequest::Top => HistoryMessage::Top,
        HistoryRequest::Bottom => HistoryMessage::Bottom(len),
        HistoryRequest::Enqueue => HistoryMessage::Enqueue(pick(
            parts.workspace,
            parts.history,
            parts.playlist,
        )),
    };
    parts.workspace.overlay.transition(OverlayMessage::Inner(
        OverlayContentMessage::History(message),
    ))
}

fn pick(
    workspace: &Workspace,
    history: &[HistoryEntry],
    playlist: &Playlist,
) -> HistoryPick {
    selected_track(workspace, history).map_or(HistoryPick::Nothing, |source| {
        playlist
            .tracks
            .iter()
            .position(|track| track.source() == source)
            .map_or(HistoryPick::Missing, |index| {
                HistoryPick::Queued(ViewIndex::new(index))
            })
    })
}

fn selected_track<'a>(
    workspace: &Workspace,
    history: &'a [HistoryEntry],
) -> Option<&'a TrackRef> {
    match &workspace.overlay {
        Some(Overlay::History(cursor)) => history
            .get(cursor.selected().get())
            .map(|entry| &entry.track),
        Some(
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::SavePlaylist { .. }
            | Overlay::Settings(..)
            | Overlay::ConfirmDelete(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::MusicDir { .. },
        )
        | None => None,
    }
}
