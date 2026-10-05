pub mod history;
pub mod jump;
mod machine;
pub mod search;
pub mod settings;
mod text_entry;

use std::{path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        cursor_over::CursorOver,
        history::{HISTORY_LIMIT, HistoryEntry},
        overlay::{DeleteCandidate, Overlay, OverlayName, SearchQuery, TextEntry},
        player::Player,
        playlist::Playlist,
        setting_row::{AppearanceSetting, SettingRow},
        track::Track,
        workspace::Workspace,
    },
    message::{HistoryRequest, OverlayRequest, SearchRequest, TextRequest},
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
    Inner(OverlayContentMessage),
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayContentMessage {
    Search(SearchRequest),
    Settings(SettingRowMessage),
    Text(TextRequest),
    Jump(TextRequest),
    History(HistoryMessage),
}

pub(crate) struct OverlayParts<'a> {
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) playlist: &'a Playlist,
    pub(crate) player: &'a Player,
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) appearance_rows: &'a [AppearanceSetting],
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
            let message =
                settings::resolve(parts.workspace, parts.appearance_rows, request)?;
            update_overlay(
                parts.workspace,
                OverlayMessage::Inner(OverlayContentMessage::Settings(message)),
            )
        }
        OverlayRequest::Text(message) => update_overlay(
            parts.workspace,
            OverlayMessage::Inner(OverlayContentMessage::Text(message)),
        ),
        OverlayRequest::Jump(message) => update_overlay(
            parts.workspace,
            OverlayMessage::Inner(OverlayContentMessage::Jump(message)),
        ),
        OverlayRequest::History(request) => {
            let len = parts.history.len();
            let message = match request {
                HistoryRequest::Navigate(direction) => {
                    HistoryMessage::Navigate { direction, len }
                }
                HistoryRequest::Top => HistoryMessage::Top,
                HistoryRequest::Bottom => HistoryMessage::Bottom(len),
                HistoryRequest::Enqueue => HistoryMessage::Enqueue(history::pick(
                    parts.workspace,
                    parts.history,
                    parts.playlist,
                )),
            };
            update_overlay(
                parts.workspace,
                OverlayMessage::Inner(OverlayContentMessage::History(message)),
            )
        }
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
    message: SearchRequest,
) -> Result<Cmd, Unhandled> {
    let edited = matches!(message, SearchRequest::Edit(_));
    let cmd = update_overlay(workspace, inner_search(message))?;
    if edited && let Some(Overlay::Search(search)) = workspace.overlay.as_mut() {
        search::rank(search, tracks);
    }
    Ok(cmd)
}

fn inner_search(message: SearchRequest) -> OverlayMessage {
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
        OverlayName::SavePlaylist => Ok(Overlay::SavePlaylist(TextEntry::default())),
        OverlayName::History => Ok(Overlay::History(CursorOver::default())),
        OverlayName::Settings => {
            Ok(Overlay::Settings(SettingRow::first(parts.appearance_rows)))
        }
        OverlayName::ConfirmDelete => {
            let track = parts
                .playlist
                .tracks
                .get(parts.workspace.browse.selected().get())
                .ok_or(Unhandled)?;
            Ok(Overlay::ConfirmDelete(DeleteCandidate {
                source: track.source().clone(),
                title: track.song_title(),
                artist: track.tags().artist.clone().unwrap_or_else(String::new),
            }))
        }
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
            input: parts.music_dir.display().to_string(),
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
