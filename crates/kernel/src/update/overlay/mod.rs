pub(crate) mod folders;
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
        catalog::{Catalog, CatalogName},
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        history::{HISTORY_LIMIT, HistoryEntry},
        overlay::{Field, Overlay, OverlayName, SearchQuery, ServerPrompt, TextEntry},
        player::Player,
        playlist::PlaylistRows,
        revision::Revisions,
        server::{Endpoint, Server},
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
    pub(crate) rows: PlaylistRows<'a>,
    pub(crate) player: &'a Player,
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) music_dir: &'a Path,
    pub(crate) servers: &'a [Server],
    pub(crate) catalog_name: &'a CatalogName,
    pub(crate) catalogs: &'a mut [Catalog],
    pub(crate) revisions: &'a mut Revisions,
}

pub(crate) fn update(
    mut parts: OverlayParts<'_>,
    request: OverlayRequest,
) -> Result<Cmd, Unhandled> {
    match request {
        OverlayRequest::Open(name) => open_request(&mut parts, name),
        OverlayRequest::Close => close_request(&mut parts),
        OverlayRequest::Confirm => confirm_request(&mut parts),
        OverlayRequest::Search(request) => search_request(&mut parts, request),
        OverlayRequest::Settings(request) => {
            let message = settings::setting_row_message(parts.workspace, request)?;
            update_overlay(
                parts.workspace,
                OverlayMessage::Content(OverlayContentMessage::Settings(message)),
            )
        }
        OverlayRequest::Text(message) => {
            if let Some(input) =
                folders::backspaced(parts.workspace.overlay.as_ref(), message)
            {
                retyped(parts.workspace, input);
                return Ok(probe(&mut parts));
            }
            let cmd = update_overlay(
                parts.workspace,
                OverlayMessage::Content(OverlayContentMessage::Text(message)),
            )?;
            Ok(cmd.then(probe(&mut parts)))
        }
        OverlayRequest::Step(direction) => {
            if let Some(Overlay::AddServer(_)) = parts.workspace.overlay {
                return navigate(&mut parts, direction);
            }
            let input = folders::stepped(parts.workspace.overlay.as_ref(), direction)
                .ok_or(Unhandled)?;
            retyped(parts.workspace, input);
            Ok(probe(&mut parts))
        }
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
    if matches!(parts.workspace.overlay, Some(Overlay::ServerSearch))
        && search::level(parts.catalogs, parts.catalog_name)
            .is_some_and(|level| level.query().is_none())
    {
        return close_request(parts);
    }
    if let Some(server) =
        selected_server(parts.workspace.overlay.as_ref(), parts.servers)
    {
        let overlay = Overlay::AddServer(ServerPrompt::from(&server.account));
        return update_overlay(parts.workspace, OverlayMessage::Open(overlay));
    }
    let from_link = from_link(parts.workspace.overlay.as_ref());
    let settled = matches!(
        parts.workspace.overlay,
        Some(Overlay::MusicDir {
            verdict: Some(_),
            revision: None,
            ..
        })
    );
    let confirmed = update_overlay(parts.workspace, OverlayMessage::Confirm)?;
    let cmd = if settled {
        confirmed.then(probe(parts))
    } else {
        confirmed
    };
    if from_link {
        fill(&mut parts.workspace.overlay, parts.servers);
    }
    Ok(cmd)
}

fn from_link(overlay: Option<&Overlay>) -> bool {
    matches!(
        overlay,
        Some(Overlay::AddServer(ServerPrompt {
            field: Field::Link,
            ..
        }))
    )
}

fn fill(overlay: &mut Option<Overlay>, servers: &[Server]) {
    if let Some(Overlay::AddServer(ServerPrompt {
        origin_server_name,
        link_text_entry,
        user_text_entry,
        ..
    })) = overlay
        && user_text_entry.input.is_empty()
        && let Ok(endpoint) = Endpoint::parse(&link_text_entry.input)
        && let Some(known) = servers.iter().find(|server| {
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
            .clone_into(&mut user_text_entry.input);
    }
}

fn retyped(workspace: &mut Workspace, input: String) {
    if let Some(Overlay::MusicDir { text_entry, .. }) = workspace.overlay.as_mut() {
        *text_entry = TextEntry { input, error: None };
    }
}

fn navigate(
    parts: &mut OverlayParts<'_>,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    if let Some(Overlay::MusicDir { folders, .. }) = parts.workspace.overlay.as_mut() {
        return folders::navigate(folders, direction);
    }
    let from_link = from_link(parts.workspace.overlay.as_ref());
    if let Some(Overlay::AddServer(server_prompt)) = parts.workspace.overlay.as_mut() {
        server_prompt.leave(direction)?;
        if from_link {
            fill(&mut parts.workspace.overlay, parts.servers);
        }
        return Ok(Cmd::none());
    }
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

fn close_request(parts: &mut OverlayParts<'_>) -> Result<Cmd, Unhandled> {
    let Some(overlay) = &parts.workspace.overlay else {
        return search::clear(parts)
            .map(|_server_query| Cmd::none())
            .ok_or(Unhandled);
    };
    let filtering = matches!(overlay, Overlay::ServerSearch);
    let cmd = update_overlay(parts.workspace, OverlayMessage::Close)?;
    if filtering {
        search::clear(parts);
    }
    Ok(cmd)
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
    Ok(load_history
        .then(update_overlay(
            parts.workspace,
            OverlayMessage::Open(opened),
        )?)
        .then(probe(parts)))
}

fn prompt(music_dir: &Path) -> Overlay {
    Overlay::MusicDir {
        text_entry: TextEntry {
            input: music_dir.to_str().map_or_else(String::new, str::to_owned),
            error: None,
        },
        verdict: None,
        revision: None,
        folders: CursorOver::default(),
    }
}

fn probe(parts: &mut OverlayParts<'_>) -> Cmd {
    let Some(Overlay::MusicDir {
        text_entry,
        verdict,
        revision,
        folders,
    }) = parts.workspace.overlay.as_mut()
    else {
        return Cmd::none();
    };
    let path = text_entry.path();
    if path.as_os_str().is_empty() {
        *verdict = None;
        *revision = None;
        *folders = CursorOver::default();
        return Cmd::none();
    }
    let issued = *revision.insert(parts.revisions.issue_effect());
    Cmd::from(Effect::Library(LibraryCmd::Probe {
        path,
        revision: issued,
    }))
    .then(folders::listing(text_entry, folders, issued))
}

fn search_request(
    parts: &mut OverlayParts<'_>,
    message: SearchRequest,
) -> Result<Cmd, Unhandled> {
    if matches!(parts.workspace.overlay, Some(Overlay::ServerSearch)) {
        return search::server_request(parts, message);
    }
    let cmd = update_overlay(parts.workspace, content_search(message))?;
    if let SearchRequest::Edit(edit) = message
        && let Some(Overlay::Search(search)) = parts.workspace.overlay.as_mut()
    {
        search::requery(search, parts.rows, edit);
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
    let track = parts.rows.get(parts.workspace.browse.selected());
    match name {
        OverlayName::Help => Ok(Overlay::Help),
        OverlayName::Search | OverlayName::ServerSearch => match parts.catalog_name {
            CatalogName::Local => {
                let matches = crate::search::rank(parts.rows, "");
                let len = matches.len();
                let query = SearchQuery {
                    input: String::new(),
                    matches,
                };
                Ok(Overlay::Search(CursorOver::new(query, len)))
            }
            CatalogName::Server(server_name) => parts
                .catalogs
                .iter()
                .any(|catalog| catalog.server_name == *server_name)
                .then_some(Overlay::ServerSearch)
                .ok_or(Unhandled),
        },
        OverlayName::SavePlaylist => Ok(Overlay::SavePlaylist(TextEntry::default())),
        OverlayName::History => Ok(Overlay::History(CursorOver::default())),
        OverlayName::Settings => Ok(Overlay::Settings(SettingRow::first())),
        OverlayName::ConfirmTrash => {
            track.cloned().map(Overlay::ConfirmTrash).ok_or(Unhandled)
        }
        OverlayName::TrackDetails => track
            .cloned()
            .or_else(|| parts.player.current().cloned())
            .map(Overlay::TrackDetails)
            .ok_or(Unhandled),
        OverlayName::JumpToTime => Ok(Overlay::JumpToTime(TextEntry::default())),
        OverlayName::MusicDir => Ok(prompt(parts.music_dir)),
        OverlayName::AddServer => Ok(Overlay::AddServer(ServerPrompt::default())),
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
            key::{Key, KeyCode, KeyPress, Modifiers},
            model::Model,
            overlay::{Field, Overlay, ServerPrompt, TextEntry},
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

    fn form(field: Field, link: &str, user: &str) -> ServerPrompt {
        ServerPrompt {
            link_text_entry: TextEntry {
                input: link.to_owned(),
                error: None,
            },
            user_text_entry: TextEntry {
                input: user.to_owned(),
                error: None,
            },
            field,
            reached_field: field,
            ..ServerPrompt::default()
        }
    }

    fn add_server(field: Field, link: &str, user: &str) -> Overlay {
        Overlay::AddServer(form(field, link, user))
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
    #[case::enter_edits_the_selected_server_in_a_form_filled_with_its_link_and_user(
        Some(servers_at(1, 2)),
        KeyCode::Enter,
        Overlay::AddServer(ServerPrompt {
            origin_server_name: Some(ServerName::new("tunes.example.com")),
            ..form(Field::Link, "https://tunes.example.com", "bob")
        })
    )]
    #[case::the_user_of_a_known_host_fills_in_on_leaving_the_link(
        Some(add_server(Field::Link, "https://tunes.example.com", "")),
        KeyCode::Enter,
        add_server(Field::User, "https://tunes.example.com", "bob")
    )]
    #[case::the_user_of_a_new_host_stays_empty_on_leaving_the_link(
        Some(add_server(Field::Link, "https://other.example.com", "")),
        KeyCode::Enter,
        add_server(Field::User, "https://other.example.com", "")
    )]
    #[case::d_asks_to_remove_the_selected_server(
        Some(servers_at(1, 2)),
        KeyCode::Char('d'),
        Overlay::ConfirmRemove(ServerName::new("tunes.example.com"))
    )]
    #[case::u_adds_a_server_from_an_empty_form(
        Some(servers_at(1, 2)),
        KeyCode::Char('u'),
        add_server(Field::Link, "", "")
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

    #[test]
    fn the_form_opens_as_clearing_an_untouched_link_leaves_it() {
        let mut opened = model_with(two_servers(), None);
        press(&mut opened, KeyCode::Char('u')).unwrap();
        let mut cleared = model_with(
            two_servers(),
            Some(add_server(Field::Link, "https://m", "")),
        );
        let key = Key {
            code: KeyCode::Char('u'),
            modifiers: Modifiers::CTRL,
        };
        update(
            &mut cleared,
            Message::Key(KeyPress { key, typed: key }),
            Moment::default(),
        )
        .unwrap();

        assert_eq!(opened.workspace.overlay, cleared.workspace.overlay);
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
