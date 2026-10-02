mod confirm_delete;
mod history;
mod jump;
mod machine;
mod search;
mod settings;
mod text;
mod track_details;

pub use history::{HistoryError, HistoryMessage, HistoryPick};
pub use jump::JumpError;
pub use search::{SearchError, SearchMessage};
pub use settings::SettingsCursorMessage;

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        CursorOver,
        Direction,
        HISTORY_LIMIT,
        JumpDigits,
        Model,
        Moment,
        Overlay,
        OverlayName,
        SearchQuery,
        SettingRow,
        TextEntry,
    },
    message::{
        BrowseRequest,
        Message,
        OverlayRequest,
        PlaybackRequest,
        PlaylistRequest,
        QueueRequest,
        SearchRequest,
        TextRequest,
    },
    update::{branch, error::UpdateError, machine::Machine},
};

#[derive(Debug)]
pub enum OverlayMessage {
    Open(Overlay),
    Close,
    Confirm,
    Inner(InnerMessage),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InnerMessage {
    Search(SearchMessage),
    Settings(SettingsCursorMessage),
    Text(TextRequest),
    Jump(TextRequest),
    History(HistoryMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OverlayError {
    #[error("no overlay is open")]
    WhileClosed,
    #[error("no track to act on")]
    NoTrack,
    #[error("a different overlay is open")]
    WrongOverlay,
    #[error("nothing to confirm")]
    NoConfirm,
    #[error("nothing selected")]
    NothingSelected,
    #[error("jump: {0}")]
    Jump(JumpError),
    #[error("search: {0}")]
    Search(SearchError),
    #[error("history: {0}")]
    History(HistoryError),
}

#[derive(Debug, PartialEq)]
pub enum FollowUp {
    Playback(PlaybackRequest),
    Browse(BrowseRequest),
    Queue(QueueRequest),
    Playlist(PlaylistRequest),
    Adjust {
        row: SettingRow,
        direction: Direction,
    },
}

impl From<FollowUp> for Message {
    fn from(follow_up: FollowUp) -> Self {
        match follow_up {
            FollowUp::Playback(message) => Self::Playback(message),
            FollowUp::Browse(message) => Self::Browse(message),
            FollowUp::Queue(message) => Self::Queue(message),
            FollowUp::Playlist(message) => Self::Playlist(message),
            FollowUp::Adjust { row, direction } => Self::Adjust { row, direction },
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct OverlayOutcome {
    pub cmd: Cmd,
    pub follow_up: Option<FollowUp>,
}

impl From<Cmd> for OverlayOutcome {
    fn from(cmd: Cmd) -> Self {
        Self {
            cmd,
            follow_up: None,
        }
    }
}

impl From<FollowUp> for OverlayOutcome {
    fn from(follow_up: FollowUp) -> Self {
        Self {
            cmd: Cmd::None,
            follow_up: Some(follow_up),
        }
    }
}

pub(crate) fn update(
    model: &mut Model,
    request: OverlayRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match request {
        OverlayRequest::Open(name) => open_request(model, name, now),
        OverlayRequest::Close => update_overlay(model, OverlayMessage::Close, now),
        OverlayRequest::Confirm => update_overlay(model, OverlayMessage::Confirm, now),
        OverlayRequest::Search(request) => search_request(model, request, now),
        OverlayRequest::Settings(request) => settings::request(model, request, now),
        OverlayRequest::Text(message) => update_overlay(
            model,
            OverlayMessage::Inner(InnerMessage::Text(message)),
            now,
        ),
        OverlayRequest::Jump(message) => update_overlay(
            model,
            OverlayMessage::Inner(InnerMessage::Jump(message)),
            now,
        ),
        OverlayRequest::History(request) => history::request(model, request, now),
    }
}

fn open_request(
    model: &mut Model,
    name: OverlayName,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let load_history = if matches!(name, OverlayName::History) {
        Effect::Library(LibraryCmd::LoadHistory {
            limit: HISTORY_LIMIT,
        })
        .into()
    } else {
        Cmd::None
    };
    let opened = overlay_for(model, name)?;
    Ok(load_history.then(update_overlay(model, OverlayMessage::Open(opened), now)?))
}

fn search_request(
    model: &mut Model,
    request: SearchRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let message = match request {
        SearchRequest::Edit(edit) => {
            SearchMessage::Edit(edit, model.playlist.tracks.clone())
        }
        SearchRequest::Navigate(direction) => SearchMessage::Navigate(direction),
        SearchRequest::Enqueue => SearchMessage::Enqueue,
    };
    update_overlay(model, inner_search(message), now)
}

fn inner_search(message: SearchMessage) -> OverlayMessage {
    OverlayMessage::Inner(InnerMessage::Search(message))
}

fn overlay_for(model: &Model, name: OverlayName) -> Result<Overlay, OverlayError> {
    match name {
        OverlayName::Help => Ok(Overlay::Help),
        OverlayName::Search => {
            let matches = crate::search::rank(&model.playlist.tracks, "");
            let len = matches.len();
            let query = SearchQuery {
                input: String::new(),
                matches,
            };
            Ok(Overlay::Search(CursorOver::new(query, len)))
        }
        OverlayName::SavePlaylist => Ok(Overlay::SavePlaylist {
            typed: TextEntry::default(),
            error: None,
        }),
        OverlayName::History => Ok(Overlay::History(CursorOver::default())),
        OverlayName::Settings => Ok(Overlay::Settings {
            selected: SettingRow::first(&model.appearance_settings),
        }),
        OverlayName::ConfirmDelete => {
            confirm_delete::candidate(&model.playlist, &model.workspace)
                .map(Overlay::ConfirmDelete)
                .ok_or(OverlayError::NoTrack)
        }
        OverlayName::TrackDetails => {
            track_details::candidate(&model.playlist, &model.player, &model.workspace)
                .map(Overlay::TrackDetails)
                .ok_or(OverlayError::NoTrack)
        }
        OverlayName::JumpToTime => Ok(Overlay::JumpToTime(JumpDigits::default())),
        OverlayName::MusicDir => Ok(Overlay::MusicDir {
            typed: TextEntry {
                input: model.music_dir.display().to_string(),
            },
            error: None,
        }),
    }
}

fn update_overlay(
    model: &mut Model,
    message: OverlayMessage,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let effect = model.workspace.overlay.transition(message)?;
    follow(model, effect, now)
}

pub(crate) fn follow(
    model: &mut Model,
    effect: OverlayOutcome,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let OverlayOutcome { cmd, follow_up } = effect;
    match follow_up {
        Some(follow_up) => Ok(cmd.then(branch(model, follow_up.into(), now)?)),
        None => Ok(cmd),
    }
}
