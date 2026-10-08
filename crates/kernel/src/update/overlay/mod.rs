pub mod history;
pub mod jump;
mod machine;
pub mod search;
pub mod settings;
mod text_entry;

use std::path::Path;

use crate::{
    cmd::{Cmd, DiskCmd, Effect, LibraryCmd},
    domain::{
        catalog::CatalogName,
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        history::{HISTORY_LIMIT, HistoryEntry},
        overlay::{
            Overlay,
            OverlayName,
            SearchQuery,
            ServerPrompt,
            ServerQuery,
            TextEntry,
        },
        player::Player,
        playlist::Playlist,
        revision::Revisions,
        server::{Server, ServerName},
        setting_row::SettingRow,
        workspace::Workspace,
    },
    message::{
        HistoryRequest,
        Message,
        OverlayRequest,
        SearchRequest,
        ServerRequest,
        TextRequest,
    },
    update::{
        machine::{Machine, Unhandled},
        overlay::{history::HistoryMessage, settings::SettingRowMessage},
        player::events::session,
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
    pub(crate) servers: &'a [Server],
    pub(crate) catalog_name: &'a CatalogName,
    pub(crate) revisions: &'a mut Revisions,
}

pub(crate) fn update(
    mut parts: OverlayParts<'_>,
    request: OverlayRequest,
) -> Result<Cmd, Unhandled> {
    match request {
        OverlayRequest::Open(name) => open_request(&mut parts, name),
        OverlayRequest::Close => update_overlay(parts.workspace, OverlayMessage::Close),
        OverlayRequest::Confirm => confirm_request(&mut parts),
        OverlayRequest::Search(request) => search_request(&mut parts, request),
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
        OverlayRequest::Navigate(direction) => navigate(&mut parts, direction),
        OverlayRequest::Reconnect => {
            selected_server(parts.workspace.overlay.as_ref(), parts.servers)
                .map(|server| {
                    Cmd::message(Message::Server(ServerRequest::Reconnect(
                        server.account.server_name.clone(),
                    )))
                })
                .ok_or(Unhandled)
        }
    }
}

fn selected_server<'a>(
    overlay: Option<&Overlay>,
    servers: &'a [Server],
) -> Option<&'a Server> {
    let Some(Overlay::Servers(cursor)) = overlay else {
        return None;
    };
    servers.get(cursor.selected().get())
}

fn confirm_request(parts: &mut OverlayParts<'_>) -> Result<Cmd, Unhandled> {
    if let Some(server) =
        selected_server(parts.workspace.overlay.as_ref(), parts.servers)
    {
        let overlay = Overlay::AddServer(ServerPrompt::Link {
            origin_server_name: Some(server.account.server_name.clone()),
            text_entry: TextEntry {
                input: server.account.endpoint.as_str().to_owned(),
                error: None,
            },
        });
        return update_overlay(parts.workspace, OverlayMessage::Open(overlay));
    }
    let from_link = matches!(
        parts.workspace.overlay,
        Some(Overlay::AddServer(ServerPrompt::Link { .. }))
    );
    let cmd = update_overlay(parts.workspace, OverlayMessage::Confirm)?;
    if from_link
        && let Some(Overlay::AddServer(ServerPrompt::User {
            origin_server_name,
            endpoint,
            text_entry,
        })) = parts.workspace.overlay.as_mut()
        && let Some(known) = parts.servers.iter().find(|server| {
            origin_server_name.as_ref().map_or_else(
                || server.account.endpoint.host() == endpoint.host(),
                |origin_server_name| server.account.server_name == *origin_server_name,
            )
        })
    {
        known
            .account
            .user_name
            .as_str()
            .clone_into(&mut text_entry.input);
    }
    Ok(cmd)
}

fn navigate(
    parts: &mut OverlayParts<'_>,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let Some(Overlay::Servers(cursor)) = parts.workspace.overlay.as_mut() else {
        return Err(Unhandled);
    };
    cursor.transition(HistoryMessage {
        request: HistoryRequest::Navigate(direction),
        rows: parts.servers.len(),
    })
}

fn servers_cursor(overlay: Option<&Overlay>, servers: &[Server]) -> CursorOver<()> {
    let top_cursor = Cursor::new(servers.len());
    let cursor = if let Some(Overlay::ConfirmRemove(server_name)) = overlay
        && let Some(index) = servers
            .iter()
            .position(|server| server.account.server_name == *server_name)
        && let Ok(rows) = isize::try_from(index)
    {
        top_cursor.step(rows)
    } else {
        top_cursor
    };
    CursorOver {
        cursor,
        content: (),
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
    parts: &mut OverlayParts<'_>,
    message: SearchRequest,
) -> Result<Cmd, Unhandled> {
    if matches!(parts.workspace.overlay, Some(Overlay::ServerSearch(_))) {
        return search::server_request(parts, message);
    }
    let cmd = update_overlay(parts.workspace, content_search(message))?;
    if let SearchRequest::Edit(edit) = message
        && let Some(Overlay::Search(search)) = parts.workspace.overlay.as_mut()
    {
        search::requery(search, &parts.playlist.tracks, edit);
    }
    Ok(cmd)
}

fn server_search(
    servers: &[Server],
    server_name: &ServerName,
) -> Result<Overlay, Unhandled> {
    session(servers, server_name).ok_or(Unhandled)?;
    let server_query = ServerQuery {
        server_name: server_name.clone(),
        input: String::new(),
        catalog_rows: Vec::new(),
        revision: None,
    };
    Ok(Overlay::ServerSearch(CursorOver::new(server_query, 0)))
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
        OverlayName::Search | OverlayName::ServerSearch => match parts.catalog_name {
            CatalogName::Local => {
                let matches = crate::search::rank(&parts.playlist.tracks, "");
                let len = matches.len();
                let query = SearchQuery {
                    input: String::new(),
                    matches,
                };
                Ok(Overlay::Search(CursorOver::new(query, len)))
            }
            CatalogName::Server(server_name) => {
                server_search(parts.servers, server_name)
            }
        },
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
        OverlayName::AddServer => Ok(Overlay::AddServer(ServerPrompt::Link {
            origin_server_name: None,
            text_entry: TextEntry::default(),
        })),
        OverlayName::Servers => Ok(Overlay::Servers(servers_cursor(
            parts.workspace.overlay.as_ref(),
            parts.servers,
        ))),
        OverlayName::ConfirmRemove => {
            selected_server(parts.workspace.overlay.as_ref(), parts.servers)
                .map(|server| {
                    Overlay::ConfirmRemove(server.account.server_name.clone())
                })
                .ok_or(Unhandled)
        }
    }
}

fn update_overlay(
    workspace: &mut Workspace,
    overlay_message: OverlayMessage,
) -> Result<Cmd, Unhandled> {
    workspace.overlay.transition(overlay_message)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        cmd::{Effect, RemoteCmd},
        domain::{
            catalog::{Catalog, CatalogName},
            cursor::Cursor,
            cursor_over::CursorOver,
            key::{Key, KeyCode, KeyPress},
            model::Model,
            overlay::{Overlay, ServerPrompt, TextEntry},
            server::{
                Account,
                Connection,
                Credential,
                Endpoint,
                RemoteError,
                Server,
                ServerName,
                ServerStatus,
                UserName,
            },
            time::Moment,
        },
        message::Message,
        update::{machine::Unhandled, update},
    };

    fn server(host: &str, user: &str) -> Server {
        let endpoint = Endpoint::parse(&format!("https://{host}")).unwrap();
        Server {
            account: Account {
                server_name: ServerName::new(endpoint.host()),
                endpoint,
                user_name: UserName::new(user).unwrap(),
            },
            server_status: ServerStatus::Offline(RemoteError::Moved {
                server_name: ServerName::new(host),
            }),
        }
    }

    fn two_servers() -> Vec<Server> {
        vec![
            server("music.example.com", "alice"),
            server("tunes.example.com", "bob"),
        ]
    }

    fn model_with(servers: Vec<Server>, overlay: Option<Overlay>) -> Model {
        let mut model = Model {
            servers,
            ..Model::default()
        };
        model.workspace.overlay = overlay;
        model
    }

    fn servers_at(steps: usize, len: usize) -> Overlay {
        let top_cursor = Cursor::new(len);
        Overlay::Servers(CursorOver {
            cursor: top_cursor.step(isize::try_from(steps).unwrap()),
            content: (),
        })
    }

    fn press(model: &mut Model, code: KeyCode) -> Result<Vec<Effect>, Unhandled> {
        let key = Key::plain(code);
        update(
            model,
            Message::Key(KeyPress { key, typed: key }),
            Moment::default(),
        )
    }

    fn link(input: &str) -> Overlay {
        Overlay::AddServer(ServerPrompt::Link {
            origin_server_name: None,
            text_entry: TextEntry {
                input: input.to_owned(),
                error: None,
            },
        })
    }

    fn user_step(link: &str, user: &str) -> Overlay {
        Overlay::AddServer(ServerPrompt::User {
            origin_server_name: None,
            endpoint: Endpoint::parse(link).unwrap(),
            text_entry: TextEntry {
                input: user.to_owned(),
                error: None,
            },
        })
    }

    #[rstest]
    #[case::c_opens_servers_on_the_first_server(
        None,
        KeyCode::Char('c'),
        servers_at(0, 2)
    )]
    #[case::j_moves_down(Some(servers_at(0, 2)), KeyCode::Char('j'), servers_at(1, 2))]
    #[case::k_moves_back_up(
        Some(servers_at(1, 2)),
        KeyCode::Char('k'),
        servers_at(0, 2)
    )]
    #[case::enter_edits_the_selected_server_at_link_with_its_endpoint_typed_in(
        Some(servers_at(1, 2)),
        KeyCode::Enter,
        Overlay::AddServer(ServerPrompt::Link {
            origin_server_name: Some(ServerName::new("tunes.example.com")),
            text_entry: TextEntry {
                input: "https://tunes.example.com".to_owned(),
                error: None,
            },
        })
    )]
    #[case::the_user_step_of_a_known_host_starts_with_its_user_typed_in(
        Some(link("https://tunes.example.com")),
        KeyCode::Enter,
        user_step("https://tunes.example.com", "bob")
    )]
    #[case::the_user_step_of_a_new_host_starts_empty(
        Some(link("https://other.example.com")),
        KeyCode::Enter,
        user_step("https://other.example.com", "")
    )]
    #[case::d_asks_to_remove_the_selected_server(
        Some(servers_at(1, 2)),
        KeyCode::Char('d'),
        Overlay::ConfirmRemove(ServerName::new("tunes.example.com"))
    )]
    #[case::u_adds_a_server_from_an_empty_link(
        Some(servers_at(1, 2)),
        KeyCode::Char('u'),
        link("")
    )]
    fn a_key_in_the_servers_overlays_opens_the_next_step(
        #[case] overlay: Option<Overlay>,
        #[case] code: KeyCode,
        #[case] expected: Overlay,
    ) {
        let mut model = model_with(two_servers(), overlay);

        press(&mut model, code).unwrap();

        assert_eq!(model.workspace.overlay, Some(expected));
    }

    fn type_text(model: &mut Model, text: &str) {
        for character in text.chars() {
            press(model, KeyCode::Char(character)).unwrap();
        }
    }

    #[test]
    fn an_edit_that_changes_the_link_replaces_the_server_it_started_from() {
        let tunes_server_name = ServerName::new("tunes.example.com");
        let beats_server_name = ServerName::new("beats.example.com");
        let mut model = model_with(two_servers(), Some(servers_at(1, 2)));
        model.catalogs = vec![Catalog::new(tunes_server_name.clone())];
        model.catalog_name = CatalogName::Server(tunes_server_name);

        press(&mut model, KeyCode::Enter).unwrap();
        for _ in "tunes.example.com".chars() {
            press(&mut model, KeyCode::Backspace).unwrap();
        }
        type_text(&mut model, "beats.example.com");
        press(&mut model, KeyCode::Enter).unwrap();
        press(&mut model, KeyCode::Enter).unwrap();
        type_text(&mut model, "secret");
        press(&mut model, KeyCode::Enter).unwrap();

        let accounts = model
            .servers
            .iter()
            .map(|server| {
                (
                    server.account.server_name.as_str(),
                    server.account.user_name.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            accounts,
            [("music.example.com", "alice"), ("beats.example.com", "bob")]
        );
        assert_eq!(
            model.catalog_name,
            CatalogName::Server(beats_server_name.clone())
        );
        assert_eq!(model.catalogs, vec![Catalog::new(beats_server_name)]);
    }

    #[test]
    fn t_reconnects_the_selected_server_with_its_stored_password() {
        let mut model = model_with(two_servers(), Some(servers_at(1, 2)));
        let account = two_servers()[1].account.clone();

        let effects = press(&mut model, KeyCode::Char('t')).unwrap();

        assert_eq!(model.servers[1].server_status, ServerStatus::Connecting);
        assert_eq!(model.servers[0], two_servers()[0]);
        assert!(
            effects.contains(&Effect::Remote(RemoteCmd::Connect(Connection {
                account,
                credential: Credential::Stored,
            })))
        );
        assert_eq!(model.workspace.overlay, Some(servers_at(1, 2)));
    }

    #[test]
    fn t_and_d_with_no_servers_are_refused_and_change_nothing() {
        for code in [KeyCode::Char('t'), KeyCode::Char('d')] {
            let mut model = model_with(Vec::new(), Some(servers_at(0, 0)));
            let before = model.clone();

            assert_eq!(press(&mut model, code), Err(Unhandled));
            assert_eq!(model, before);
        }
    }

    #[test]
    fn enter_in_confirm_remove_drops_the_server_and_closes() {
        let mut model = model_with(
            two_servers(),
            Some(Overlay::ConfirmRemove(ServerName::new("music.example.com"))),
        );

        press(&mut model, KeyCode::Enter).unwrap();

        assert_eq!(model.servers, vec![server("tunes.example.com", "bob")]);
        assert_eq!(model.workspace.overlay, None);
    }

    #[test]
    fn esc_in_confirm_remove_goes_back_to_servers_on_the_same_server() {
        let mut model = model_with(
            two_servers(),
            Some(Overlay::ConfirmRemove(ServerName::new("tunes.example.com"))),
        );

        press(&mut model, KeyCode::Esc).unwrap();

        assert_eq!(model.servers, two_servers());
        assert_eq!(model.workspace.overlay, Some(servers_at(1, 2)));
    }
}
