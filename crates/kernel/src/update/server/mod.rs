pub(crate) mod catalog;
mod download;

use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect, RemoteCmd},
    domain::{
        catalog::{Catalog, CatalogName, Paging},
        favorites::Favorites,
        overlay::{Field, Overlay, ServerPrompt},
        player::Player,
        playlist::{Playlist, PlaylistSource},
        revision::Revisions,
        server::{
            Connection,
            Credential,
            Download,
            PlayReport,
            RemoteError,
            Server,
            ServerName,
            ServerStatus,
        },
        toast::Toast,
        workspace::Workspace,
    },
    message::{Message, RemoteEvent, ServerRequest},
    update::{
        machine::{Machine, Unhandled, replace},
        overlay::OverlayMessage,
        play_reports,
    },
};

pub(crate) struct ServerParts<'a> {
    pub(crate) servers: &'a mut Vec<Server>,
    pub(crate) downloads: &'a mut Vec<Download>,
    pub(crate) player: &'a mut Player,
    pub(crate) catalog_name: &'a mut CatalogName,
    pub(crate) catalogs: &'a mut Vec<Catalog>,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) favorites: &'a mut Favorites,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) playlist_source: &'a PlaylistSource,
    pub(crate) play_reports: &'a mut Vec<PlayReport>,
}

pub(crate) fn request(
    server_parts: ServerParts<'_>,
    request: ServerRequest,
) -> Result<Cmd, Unhandled> {
    match request {
        ServerRequest::Add {
            connection,
            origin_server_name,
        } => Ok(add(server_parts, connection, origin_server_name.as_ref())),
        ServerRequest::Reconnect(server_name) => {
            let server = server_parts
                .servers
                .iter_mut()
                .find(|server| server.account.server_name == server_name)
                .ok_or(Unhandled)?;
            server.server_status = ServerStatus::Connecting;
            Ok(Cmd::from(Effect::Remote(RemoteCmd::Connect(Connection {
                account: server.account.clone(),
                credential: Credential::Stored,
            }))))
        }
        ServerRequest::Remove(server_name) => {
            let ServerParts {
                servers,
                downloads,
                player,
                catalog_name,
                catalogs,
                revisions: _,
                favorites: _,
                workspace: _,
                playlist: _,
                playlist_source: _,
                play_reports: kept_play_reports,
            } = server_parts;
            let index = servers
                .iter()
                .position(|server| server.account.server_name == server_name)
                .ok_or(Unhandled)?;
            let effect =
                Effect::Remote(RemoteCmd::Forget(servers.remove(index).account));
            catalogs.retain(|catalog| catalog.server_name != server_name);
            play_reports::forget(kept_play_reports, &server_name);
            let stop = download::forget(downloads, player, &server_name);
            if *catalog_name == CatalogName::Server(server_name) {
                *catalog_name = CatalogName::Local;
            }
            Ok(Cmd::from_iter([save_accounts(servers), effect]).then(stop))
        }
    }
}

fn add(
    server_parts: ServerParts<'_>,
    connection: Connection,
    origin_server_name: Option<&ServerName>,
) -> Cmd {
    let ServerParts {
        servers,
        downloads: _,
        player: _,
        catalog_name,
        catalogs,
        revisions: _,
        favorites: _,
        workspace: _,
        playlist: _,
        playlist_source: _,
        play_reports: _,
    } = server_parts;
    let account = &connection.account;
    let warning = if account.endpoint.is_https() {
        Cmd::none()
    } else {
        Cmd::message(Message::Toast(Toast::info(format!(
            "{} uses http://, so its sign-in travels unencrypted",
            account.server_name
        ))))
    };
    let server_name = &account.server_name;
    let replaced =
        |known: &ServerName| known == server_name || origin_server_name == Some(known);
    let index = servers
        .iter()
        .position(|known| replaced(&known.account.server_name))
        .unwrap_or(servers.len());
    servers.retain(|known| !replaced(&known.account.server_name));
    servers.insert(
        index,
        Server {
            account: account.clone(),
            server_status: ServerStatus::Connecting,
        },
    );
    catalogs.retain(|known| !replaced(&known.server_name));
    catalogs.push(Catalog::new(server_name.clone()));
    if matches!(catalog_name, CatalogName::Server(open) if replaced(open)) {
        *catalog_name = CatalogName::Server(server_name.clone());
    }
    let effect = Effect::Remote(RemoteCmd::Connect(connection));
    Cmd::from_iter([effect, save_accounts(servers)]).then(warning)
}

fn save_accounts(servers: &[Server]) -> Effect {
    let accounts = servers.iter().map(|server| server.account.clone());
    Effect::Config(ConfigCmd::Save(ConfigPatch {
        accounts: Some(accounts.collect()),
        ..ConfigPatch::default()
    }))
}

pub(crate) fn update(
    mut server_parts: ServerParts<'_>,
    event: RemoteEvent,
) -> Result<Cmd, Unhandled> {
    match event {
        RemoteEvent::Connected {
            server_name,
            session,
        } => status(server_parts, &server_name, ServerStatus::Online(session)),
        RemoteEvent::Error(error) => status(
            server_parts,
            &error.server_name().clone(),
            ServerStatus::Offline(error),
        ),
        RemoteEvent::Fetched { revision, result } => {
            download::fetched(server_parts, revision, result)
        }
        RemoteEvent::Listed(catalog_page) => {
            catalog::listed(&mut server_parts, catalog_page)
        }
        RemoteEvent::Found {
            result, revision, ..
        } => catalog::found(&mut server_parts, result, revision),
        RemoteEvent::Starred(server_favorite) => {
            catalog::starred(&mut server_parts, server_favorite)
        }
        RemoteEvent::Restored(_)
        | RemoteEvent::Unsaved(_)
        | RemoteEvent::Cover { .. } => Err(Unhandled),
    }
}

fn status(
    server_parts: ServerParts<'_>,
    server_name: &ServerName,
    server_status: ServerStatus,
) -> Result<Cmd, Unhandled> {
    let ServerParts {
        servers,
        downloads: _,
        player: _,
        catalog_name,
        catalogs,
        revisions,
        favorites: _,
        workspace,
        playlist: _,
        playlist_source: _,
        play_reports: _,
    } = server_parts;
    let server = servers
        .iter_mut()
        .find(|server| server.account.server_name == *server_name)
        .ok_or(Unhandled)?;
    replace(&mut server.server_status, server_status)?;
    let answer =
        if let Some(Overlay::AddServer(server_prompt)) = workspace.overlay.as_mut() {
            server_prompt.answered(server_name, &server.server_status)
        } else {
            None
        };
    let toast = match &server.server_status {
        ServerStatus::Offline(error) => {
            let levels = catalogs
                .iter_mut()
                .filter(|catalog| catalog.server_name == *server_name)
                .flat_map(Catalog::levels);
            for level in levels {
                match level.paging {
                    Paging::Loading(page) => level.paging = Paging::Queued(page),
                    Paging::Next(_) | Paging::Queued(_) | Paging::Complete => {}
                }
            }
            match answer {
                Some(answer) => answer,
                None => offline(workspace, server, error)?,
            }
        }
        ServerStatus::Connecting => Cmd::none(),
        ServerStatus::Online(_) => {
            let catalog = catalogs
                .iter_mut()
                .find(|catalog| match catalog_name {
                    CatalogName::Server(shown_name) => {
                        *shown_name == catalog.server_name
                    }
                    CatalogName::Local => false,
                })
                .filter(|catalog| catalog.server_name == *server_name);
            if let Some(catalog) = catalog {
                catalog::show(catalog);
            }
            answer.unwrap_or_else(Cmd::none)
        }
    };
    Ok(toast.then(catalog::list(catalogs, servers, revisions)))
}

fn offline(
    workspace: &mut Workspace,
    server: &Server,
    error: &RemoteError,
) -> Result<Cmd, Unhandled> {
    if matches!(error, RemoteError::NoPassword { .. }) && workspace.overlay.is_none() {
        let server_prompt = ServerPrompt {
            field: Field::Password,
            ..ServerPrompt::from(&server.account)
        };
        return workspace
            .overlay
            .transition(OverlayMessage::Open(Overlay::AddServer(server_prompt)));
    }
    Ok(Cmd::message(Message::Toast(Toast::error(
        error.to_string(),
    ))))
}

pub(crate) fn online(server_status: &ServerStatus) -> Result<(), Unhandled> {
    match server_status {
        ServerStatus::Online(_) => Ok(()),
        ServerStatus::Connecting | ServerStatus::Offline(_) => Err(Unhandled),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        cmd::Cmd,
        domain::{
            io_error::IoError,
            model::Model,
            server::{
                Account,
                Download,
                Endpoint,
                RemoteError,
                Server,
                ServerName,
                ServerStatus,
                Session,
                UserName,
            },
            toast::Toast,
        },
        message::{Message, RemoteEvent},
        update::{machine::Unhandled, server::update, server_parts},
    };

    pub(crate) fn server(server_status: ServerStatus) -> Server {
        Server {
            account: Account {
                server_name: ServerName::new("home"),
                endpoint: Endpoint::parse("https://music.example.com").unwrap(),
                user_name: UserName::new("ann").unwrap(),
            },
            server_status,
        }
    }

    pub(crate) fn model_with(servers: Vec<Server>, downloads: Vec<Download>) -> Model {
        Model {
            servers,
            downloads,
            ..Model::default()
        }
    }

    pub(crate) fn unreachable(name: &str) -> RemoteError {
        RemoteError::Unreachable {
            server_name: ServerName::new(name),
            source: IoError::Other,
        }
    }

    pub(crate) fn session() -> Session {
        Session::new(
            Endpoint::parse("https://music.example.com").unwrap(),
            "u=ann&t=token&s=salt",
        )
    }

    struct RemoteRow {
        server_status: ServerStatus,
        event: RemoteEvent,
        expected: Result<Cmd, Unhandled>,
        expected_server_status: ServerStatus,
    }

    #[rstest]
    #[case::connected(RemoteRow {
        server_status: ServerStatus::Connecting,
        event: RemoteEvent::Connected {
            server_name: ServerName::new("home"),
            session: session(),
        },
        expected: Ok(Cmd::none()),
        expected_server_status: ServerStatus::Online(session()),
    })]
    #[case::unreachable(RemoteRow {
        server_status: ServerStatus::Online(session()),
        event: RemoteEvent::Error(unreachable("home")),
        expected: Ok(Cmd::message(Message::Toast(Toast::error(unreachable("home").to_string())))),
        expected_server_status: ServerStatus::Offline(unreachable("home")),
    })]
    #[case::same_error_again(RemoteRow {
        server_status: ServerStatus::Offline(unreachable("home")),
        event: RemoteEvent::Error(unreachable("home")),
        expected: Err(Unhandled),
        expected_server_status: ServerStatus::Offline(unreachable("home")),
    })]
    #[case::unknown_server_connected(RemoteRow {
        server_status: ServerStatus::Connecting,
        event: RemoteEvent::Connected {
            server_name: ServerName::new("elsewhere"),
            session: session(),
        },
        expected: Err(Unhandled),
        expected_server_status: ServerStatus::Connecting,
    })]
    #[case::unknown_server_error(RemoteRow {
        server_status: ServerStatus::Connecting,
        event: RemoteEvent::Error(unreachable("elsewhere")),
        expected: Err(Unhandled),
        expected_server_status: ServerStatus::Connecting,
    })]
    fn a_remote_event_moves_the_server_status(#[case] row: RemoteRow) {
        let mut model = model_with(vec![server(row.server_status)], Vec::new());

        let answer = update(server_parts(&mut model), row.event);

        assert_eq!(answer, row.expected);
        assert_eq!(model.servers, vec![server(row.expected_server_status)]);
    }
}
