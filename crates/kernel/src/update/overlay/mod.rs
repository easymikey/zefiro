mod confirm_delete;
mod history;
mod jump;
mod machine;
mod search;
mod settings;
mod text;
mod track_details;

use std::{path::Path, sync::Arc};

pub use history::{HistoryMessage, HistoryPick};
pub use jump::JumpDigitsMessage;
pub use search::SearchQueryMessage;
pub use settings::SettingRowMessage;

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        AppearanceSetting,
        CursorOver,
        HISTORY_LIMIT,
        HistoryEntry,
        JumpDigits,
        Overlay,
        OverlayName,
        Player,
        SearchQuery,
        SettingRow,
        TextEntry,
        Track,
        Workspace,
        playlist::Playlist,
    },
    message::{OverlayRequest, SearchRequest, TextRequest},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug)]
pub enum OverlayMessage {
    Open(Overlay),
    Close,
    Confirm,
    Inner(OverlayContentMessage),
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayContentMessage {
    Search(SearchQueryMessage),
    Settings(SettingRowMessage),
    Text(TextRequest),
    Jump(JumpDigitsMessage),
    History(HistoryMessage),
}

pub(crate) struct OverlayParts<'a> {
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) playlist: &'a Playlist,
    pub(crate) player: &'a Player,
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) appearance_settings: &'a [AppearanceSetting],
    pub(crate) music_dir: &'a Path,
}

pub(crate) fn update(
    mut parts: OverlayParts<'_>,
    request: OverlayRequest,
) -> Result<Cmd, Unhandled> {
    match request {
        OverlayRequest::Open(name) => open_request(&mut parts, name),
        OverlayRequest::Close => update_overlay(parts.workspace, OverlayMessage::Close),
        OverlayRequest::Confirm => {
            update_overlay(parts.workspace, OverlayMessage::Confirm)
        }
        OverlayRequest::Search(request) => {
            search_request(parts.workspace, &parts.playlist.tracks, request)
        }
        OverlayRequest::Settings(request) => {
            settings::request(parts.workspace, parts.appearance_settings, request)
        }
        OverlayRequest::Text(message) => update_overlay(
            parts.workspace,
            OverlayMessage::Inner(OverlayContentMessage::Text(message)),
        ),
        OverlayRequest::Jump(message) => update_overlay(
            parts.workspace,
            OverlayMessage::Inner(OverlayContentMessage::Jump(
                JumpDigitsMessage::from(message),
            )),
        ),
        OverlayRequest::History(request) => history::request(&mut parts, request),
    }
}

fn open_request(
    parts: &mut OverlayParts<'_>,
    name: OverlayName,
) -> Result<Cmd, Unhandled> {
    let load_history = if matches!(name, OverlayName::History) {
        Effect::Library(LibraryCmd::LoadHistory(HISTORY_LIMIT)).into()
    } else {
        Cmd::none()
    };
    let opened = overlay_for(parts, name)?;
    Ok(load_history.then(update_overlay(
        parts.workspace,
        OverlayMessage::Open(opened),
    )?))
}

fn search_request(
    workspace: &mut Workspace,
    tracks: &[Arc<Track>],
    request: SearchRequest,
) -> Result<Cmd, Unhandled> {
    let message = match request {
        SearchRequest::Edit(edit) => SearchQueryMessage::Edit(edit),
        SearchRequest::Navigate(direction) => SearchQueryMessage::Navigate(direction),
        SearchRequest::Enqueue => SearchQueryMessage::Enqueue,
    };
    let edited = matches!(message, SearchQueryMessage::Edit(_));
    let cmd = update_overlay(workspace, inner_search(message))?;
    if edited && let Some(Overlay::Search(search)) = workspace.overlay.as_mut() {
        search::rank(search, tracks);
    }
    Ok(cmd)
}

fn inner_search(message: SearchQueryMessage) -> OverlayMessage {
    OverlayMessage::Inner(OverlayContentMessage::Search(message))
}

fn overlay_for(
    parts: &OverlayParts<'_>,
    name: OverlayName,
) -> Result<Overlay, Unhandled> {
    match name {
        OverlayName::Help => Ok(Overlay::Help),
        OverlayName::Search => {
            let matches = crate::search::rank(&parts.playlist.tracks, "");
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
        OverlayName::Settings => Ok(Overlay::Settings(SettingRow::first(
            parts.appearance_settings,
        ))),
        OverlayName::ConfirmDelete => {
            confirm_delete::candidate(parts.playlist, parts.workspace)
                .map(Overlay::ConfirmDelete)
                .ok_or(Unhandled)
        }
        OverlayName::TrackDetails => {
            track_details::candidate(parts.playlist, parts.player, parts.workspace)
                .map(Overlay::TrackDetails)
                .ok_or(Unhandled)
        }
        OverlayName::JumpToTime => Ok(Overlay::JumpToTime(JumpDigits::default())),
        OverlayName::MusicDir => Ok(Overlay::MusicDir {
            typed: TextEntry {
                input: parts.music_dir.display().to_string(),
            },
            error: None,
        }),
    }
}

fn update_overlay(
    workspace: &mut Workspace,
    message: OverlayMessage,
) -> Result<Cmd, Unhandled> {
    workspace.overlay.transition(message)
}
