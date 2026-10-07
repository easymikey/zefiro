pub mod history;
pub mod jump;
mod machine;
pub mod search;
pub mod settings;
mod text_entry;

use std::{path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, DiskCmd, Effect, LibraryCmd},
    domain::{
        cursor_over::CursorOver,
        history::{HISTORY_LIMIT, HistoryEntry},
        overlay::{Overlay, OverlayName, SearchQuery, TextEntry},
        player::Player,
        playlist::Playlist,
        setting_row::SettingRow,
        track::Track,
        workspace::Workspace,
    },
    message::{OverlayRequest, SearchRequest, TextRequest},
    update::{
        machine::{Machine, Unhandled},
        overlay::{history::HistoryMessage, settings::SettingRowMessage},
    },
};

#[derive(Debug)]
pub enum OverlayMessage {
    Open(Overlay),
    Close,
    Confirm,
    Content(OverlayContentMessage),
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayContentMessage {
    Search(SearchRequest),
    Settings(SettingRowMessage),
    Text(TextRequest),
    History(HistoryMessage),
}

pub(crate) struct OverlayParts<'a> {
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) playlist: &'a Playlist,
    pub(crate) player: &'a Player,
    pub(crate) history: &'a [HistoryEntry],
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
            let message = settings::setting_row_message(parts.workspace, request)?;
            update_overlay(
                parts.workspace,
                OverlayMessage::Content(OverlayContentMessage::Settings(message)),
            )
        }
        OverlayRequest::Text(message) => update_overlay(
            parts.workspace,
            OverlayMessage::Content(OverlayContentMessage::Text(message)),
        ),
        OverlayRequest::History(request) => update_overlay(
            parts.workspace,
            OverlayMessage::Content(OverlayContentMessage::History(HistoryMessage {
                request,
                rows: parts.history.len(),
            })),
        ),
    }
}

fn open_request(
    parts: &mut OverlayParts<'_>,
    name: OverlayName,
) -> Result<Cmd, Unhandled> {
    let load_history = if matches!(name, OverlayName::History) {
        Effect::Library(LibraryCmd::Disk(DiskCmd::LoadHistory(HISTORY_LIMIT))).into()
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
    message: SearchRequest,
) -> Result<Cmd, Unhandled> {
    let cmd = update_overlay(workspace, content_search(message))?;
    if let SearchRequest::Edit(edit) = message
        && let Some(Overlay::Search(search)) = workspace.overlay.as_mut()
    {
        search::requery(search, tracks, edit);
    }
    Ok(cmd)
}

fn content_search(message: SearchRequest) -> OverlayMessage {
    OverlayMessage::Content(OverlayContentMessage::Search(message))
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
        OverlayName::SavePlaylist => Ok(Overlay::SavePlaylist(TextEntry::default())),
        OverlayName::History => Ok(Overlay::History(CursorOver::default())),
        OverlayName::Settings => Ok(Overlay::Settings(SettingRow::first())),
        OverlayName::ConfirmTrash => parts
            .playlist
            .tracks
            .get(parts.workspace.browse.selected().get())
            .cloned()
            .map(Overlay::ConfirmTrash)
            .ok_or(Unhandled),
        OverlayName::TrackDetails => parts
            .playlist
            .tracks
            .get(parts.workspace.browse.selected().get())
            .cloned()
            .or_else(|| parts.player.current().cloned())
            .map(Overlay::TrackDetails)
            .ok_or(Unhandled),
        OverlayName::JumpToTime => Ok(Overlay::JumpToTime(TextEntry::default())),
        OverlayName::MusicDir => Ok(Overlay::MusicDir(TextEntry {
            input: parts
                .music_dir
                .to_str()
                .map_or_else(String::new, str::to_owned),
            error: None,
        })),
    }
}

fn update_overlay(
    workspace: &mut Workspace,
    message: OverlayMessage,
) -> Result<Cmd, Unhandled> {
    workspace.overlay.transition(message)
}
