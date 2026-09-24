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
pub use settings::{SettingsMessage, SettingsRejection};

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        CursorOver,
        JumpDigits,
        Model,
        Nudge,
        Overlay,
        OverlayName,
        SearchQuery,
        SettingRow,
        SettingsRows,
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
    Settings(SettingsRejection),
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

pub(super) fn update(
    model: &mut Model,
    request: OverlayRequest,
) -> Result<Cmd, Rejection> {
    match request {
        OverlayRequest::Open(name) => open_request(model, name),
        OverlayRequest::Close => update_overlay(model, OverlayMessage::Close),
        OverlayRequest::Confirm => update_overlay(model, OverlayMessage::Confirm),
        OverlayRequest::Search(request) => search_request(model, request),
        OverlayRequest::Settings(request) => settings::request(model, request),
        OverlayRequest::Text(message) => {
            update_overlay(model, OverlayMessage::Inner(InnerMessage::Text(message)))
        }
        OverlayRequest::Jump(message) => {
            update_overlay(model, OverlayMessage::Inner(InnerMessage::Jump(message)))
        }
        OverlayRequest::History(request) => history::request(model, request),
    }
}

fn open_request(model: &mut Model, name: OverlayName) -> Result<Cmd, Rejection> {
    let (opened, cmd) = overlay_for(model, name)?;
    Ok(cmd.then(update_overlay(model, OverlayMessage::Open(opened))?))
}

fn search_request(model: &mut Model, request: SearchRequest) -> Result<Cmd, Rejection> {
    match request {
        SearchRequest::Edit(edit) => {
            let tracks = model.playlist.tracks.clone();
            update_overlay(model, inner_search(SearchMessage::Edit(edit, tracks)))
        }
        SearchRequest::Navigate(nudge) => {
            update_overlay(model, inner_search(SearchMessage::Navigate(nudge)))
        }
        SearchRequest::Enqueue => {
            update_overlay(model, inner_search(SearchMessage::Enqueue))
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
        OverlayName::Settings => {
            let len = SettingRow::all(&model.custom_rows).len();
            Ok((
                Overlay::Settings(CursorOver::new(SettingsRows, len)),
                Cmd::None,
            ))
        }
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
) -> Result<Cmd, Rejection> {
    let effect = model.workspace.overlay.update(message)?;
    follow(model, effect)
}

fn follow(model: &mut Model, effect: OverlayEffect) -> Result<Cmd, Rejection> {
    let OverlayEffect { cmd, follow_up } = effect;
    match follow_up {
        Some(follow_up) => Ok(cmd.then(branch(model, follow_up.into())?)),
        None => Ok(cmd),
    }
}

pub(super) fn selected_setting_row(model: &Model) -> Option<SettingRow> {
    let Some(Overlay::Settings(cursor)) = &model.workspace.overlay else {
        return None;
    };
    SettingRow::all(&model.custom_rows)
        .get(cursor.selected())
        .copied()
}
