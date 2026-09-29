mod confirm_delete;
mod history;
mod jump;
mod machine;
mod search;
mod settings;
mod text;
mod track_details;

pub use history::{HistoryMessage, HistoryPick, HistoryRejection};
pub use jump::JumpRejection;
pub use search::{SearchMessage, SearchRejection};
pub use settings::SettingsMessage;

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        CursorOver,
        JumpDigits,
        Model,
        Moment,
        Nudge,
        Overlay,
        OverlayName,
        SearchQuery,
        SettingRow,
        SettingsCursor,
        TextEntry,
    },
    message::{
        BrowseRequest,
        JumpRequest,
        LoadedRequest,
        Message,
        OverlayRequest,
        PlaybackRequest,
        SearchRequest,
        TextRequest,
    },
    update::{branch, machine::Machine, rejection::Rejection},
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
    Settings(SettingsMessage),
    Text(TextRequest),
    Jump(JumpRequest),
    History(HistoryMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayRejection {
    WhileClosed,
    NoTrack,
    WrongOverlay,
    NoConfirm,
    NothingSelected,
    Jump(JumpRejection),
    Search(SearchRejection),
    History(HistoryRejection),
}

#[derive(Debug, PartialEq)]
pub enum FollowUp {
    Playback(PlaybackRequest),
    Browse(BrowseRequest),
    Loaded(LoadedRequest),
    Adjust { row: SettingRow, nudge: Nudge },
}

impl From<FollowUp> for Message {
    fn from(follow_up: FollowUp) -> Self {
        match follow_up {
            FollowUp::Playback(message) => Self::Playback(message),
            FollowUp::Browse(message) => Self::Browse(message),
            FollowUp::Loaded(message) => Self::Loaded(message),
            FollowUp::Adjust { row, nudge } => Self::Adjust { row, nudge },
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct OverlayEffect {
    pub cmd: Cmd,
    pub follow_up: Option<FollowUp>,
}

impl From<Cmd> for OverlayEffect {
    fn from(cmd: Cmd) -> Self {
        Self {
            cmd,
            follow_up: None,
        }
    }
}

impl From<FollowUp> for OverlayEffect {
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
) -> Result<Cmd, Rejection> {
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
) -> Result<Cmd, Rejection> {
    let (opened, cmd) = overlay_for(model, name)?;
    Ok(cmd.then(update_overlay(model, OverlayMessage::Open(opened), now)?))
}

fn search_request(
    model: &mut Model,
    request: SearchRequest,
    now: Moment,
) -> Result<Cmd, Rejection> {
    match request {
        SearchRequest::Edit(edit) => {
            let tracks = model.playlist.tracks.clone();
            update_overlay(model, inner_search(SearchMessage::Edit(edit, tracks)), now)
        }
        SearchRequest::Navigate(nudge) => {
            update_overlay(model, inner_search(SearchMessage::Navigate(nudge)), now)
        }
        SearchRequest::Enqueue => {
            update_overlay(model, inner_search(SearchMessage::Enqueue), now)
        }
    }
}

fn inner_search(message: SearchMessage) -> OverlayMessage {
    OverlayMessage::Inner(InnerMessage::Search(message))
}

fn overlay_for(
    model: &Model,
    name: OverlayName,
) -> Result<(Overlay, Cmd), OverlayRejection> {
    match name {
        OverlayName::Help => Ok((Overlay::Help, Cmd::None)),
        OverlayName::Search => {
            let matches = crate::search::rank(&model.playlist.tracks, "");
            let len = matches.len();
            let query = SearchQuery {
                input: String::new(),
                matches,
            };
            Ok((Overlay::Search(CursorOver::new(query, len)), Cmd::None))
        }
        OverlayName::SavePlaylist => Ok((
            Overlay::SavePlaylist {
                typed: TextEntry::default(),
                error: None,
            },
            Cmd::None,
        )),
        OverlayName::History => Ok((
            Overlay::History(CursorOver::default()),
            Effect::Library(LibraryCmd::LoadHistory {
                limit: model.history.view_cap,
            })
            .into(),
        )),
        OverlayName::Settings => Ok((
            Overlay::Settings(SettingsCursor::first(&model.custom_rows)),
            Cmd::None,
        )),
        OverlayName::ConfirmDelete => {
            confirm_delete::candidate(&model.playlist, &model.workspace)
                .map(|candidate| (Overlay::ConfirmDelete(candidate), Cmd::None))
                .ok_or(OverlayRejection::NoTrack)
        }
        OverlayName::TrackDetails => {
            track_details::candidate(&model.playlist, &model.player, &model.workspace)
                .map(|track| (Overlay::TrackDetails(track), Cmd::None))
                .ok_or(OverlayRejection::NoTrack)
        }
        OverlayName::JumpToTime => {
            Ok((Overlay::JumpToTime(JumpDigits::default()), Cmd::None))
        }
        OverlayName::SourceDir => Ok((
            Overlay::SourceDir {
                typed: TextEntry {
                    input: model.music_dir.display().to_string(),
                },
                error: None,
            },
            Cmd::None,
        )),
    }
}

fn update_overlay(
    model: &mut Model,
    message: OverlayMessage,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let effect = model.workspace.overlay.update(message)?;
    follow(model, effect, now)
}

pub(crate) fn follow(
    model: &mut Model,
    effect: OverlayEffect,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let OverlayEffect { cmd, follow_up } = effect;
    match follow_up {
        Some(follow_up) => Ok(cmd.then(branch(model, follow_up.into(), now)?)),
        None => Ok(cmd),
    }
}
