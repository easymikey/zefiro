use std::time::Duration;

use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        catalog::{BrowseLevel, Catalog, CatalogName, PAGE_LEAD, Paging},
        cursor::Cursor,
        cursor_over::cycled,
        direction::Direction,
        player::Player,
        revision::{Freshness, Revision, Revisions},
        server::{
            Connection,
            Credential,
            Download,
            Fetched,
            Listing,
            MediaFetch,
            PAGE_ROWS,
            Page,
            RemoteError,
            Server,
            ServerName,
            ServerStatus,
        },
        toast::Toast,
        track::{CatalogRow, TrackSource},
    },
    message::{Message, RemoteEvent, ServerRequest, Timer},
    update::machine::{Unhandled, replace},
};

pub(crate) const FETCH_RETRY: Duration = Duration::from_secs(5);

pub(crate) struct ServerParts<'a> {
    pub(crate) servers: &'a mut Vec<Server>,
    pub(crate) downloads: &'a mut [Download],
    pub(crate) player: &'a Player,
    pub(crate) catalog_name: &'a mut CatalogName,
    pub(crate) catalogs: &'a mut Vec<Catalog>,
    pub(crate) revisions: &'a mut Revisions,
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
                downloads: _downloads,
                player: _player,
                catalog_name,
                catalogs,
                revisions: _revisions,
            } = server_parts;
            let index = servers
                .iter()
                .position(|server| server.account.server_name == server_name)
                .ok_or(Unhandled)?;
            servers.remove(index);
            catalogs.retain(|catalog| catalog.server_name != server_name);
            if matches!(catalog_name, CatalogName::Server(open) if *open == server_name)
            {
                *catalog_name = CatalogName::Local;
            }
            Ok(Cmd::none())
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
        downloads: _downloads,
        player: _player,
        catalog_name,
        catalogs,
        revisions: _revisions,
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
    Cmd::from(Effect::Remote(RemoteCmd::Connect(connection))).then(warning)
}

pub(crate) fn update(
    server_parts: ServerParts<'_>,
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
        RemoteEvent::Listed {
            server_name,
            listing,
            page,
            catalog_rows,
            revision,
        } => {
            match revision.freshness(server_parts.revisions.list) {
                Freshness::Awaited => {}
                Freshness::Stale => return Err(Unhandled),
            }
            let level = server_parts
                .catalogs
                .iter_mut()
                .find(|catalog| catalog.server_name == server_name)
                .map(level)
                .ok_or(Unhandled)?;
            if level.listing != listing || level.paging != Paging::Loading(page) {
                return Err(Unhandled);
            }
            listed(level, page, catalog_rows);
            Ok(Cmd::none())
        }
        RemoteEvent::Fetched { revision, result } => {
            fetched(server_parts, revision, result)
        }
        RemoteEvent::Found {
            server_name: _server_name,
            catalog_rows: _catalog_rows,
            revision: _revision,
        } => Err(Unhandled),
    }
}

fn status(
    server_parts: ServerParts<'_>,
    server_name: &ServerName,
    server_status: ServerStatus,
) -> Result<Cmd, Unhandled> {
    let ServerParts {
        servers,
        downloads: _downloads,
        player: _player,
        catalog_name,
        catalogs,
        revisions,
    } = server_parts;
    let server = servers
        .iter_mut()
        .find(|server| server.account.server_name == *server_name)
        .ok_or(Unhandled)?;
    replace(&mut server.server_status, server_status)?;
    Ok(match &server.server_status {
        ServerStatus::Offline(error) => {
            Cmd::message(Message::Toast(Toast::error(error.to_string())))
        }
        ServerStatus::Connecting => Cmd::none(),
        ServerStatus::Online(_) => catalogs
            .iter_mut()
            .find(|catalog| match catalog_name {
                CatalogName::Server(shown_name) => *shown_name == catalog.server_name,
                CatalogName::Local => false,
            })
            .filter(|catalog| catalog.server_name == *server_name)
            .map_or_else(Cmd::none, |catalog| {
                show(catalog, &server.server_status, revisions)
            }),
    })
}

fn fetched(
    server_parts: ServerParts<'_>,
    revision: Revision,
    result: Result<Fetched, RemoteError>,
) -> Result<Cmd, Unhandled> {
    let ServerParts {
        servers,
        downloads,
        player,
        catalog_name: _catalog_name,
        catalogs: _catalogs,
        revisions: _revisions,
    } = server_parts;
    let download = downloads
        .iter_mut()
        .find(|download| download.media_fetch.revision == revision)
        .ok_or(Unhandled)?;
    match result {
        Ok(fetched) => {
            let online = servers
                .iter()
                .find(|server| {
                    server.account.server_name == download.media_fetch.server_name
                })
                .filter(|server| {
                    matches!(server.server_status, ServerStatus::Offline(_))
                })
                .map_or_else(Cmd::none, |_server| {
                    Cmd::message(Message::Remote(RemoteEvent::Connected {
                        server_name: download.media_fetch.server_name.clone(),
                        session: download.media_fetch.session.clone(),
                    }))
                });
            download.fetched = Some(fetched);
            Ok(chunk(download, player)
                .map_or_else(Cmd::none, |remote_cmd| {
                    Cmd::from(Effect::Remote(remote_cmd))
                })
                .then(online))
        }
        Err(error) => Ok(Cmd::from(Effect::After {
            delay: FETCH_RETRY,
            timer: Timer::Fetch(revision),
        })
        .then(Cmd::message(Message::Remote(RemoteEvent::Error(error))))),
    }
}

pub(crate) fn retry(
    downloads: &[Download],
    player: &Player,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    downloads
        .iter()
        .find(|download| download.media_fetch.revision == revision)
        .and_then(|download| chunk(download, player))
        .map(|remote_cmd| Cmd::from(Effect::Remote(remote_cmd)))
        .ok_or(Unhandled)
}

pub(crate) fn chunk(download: &Download, player: &Player) -> Option<RemoteCmd> {
    let media_fetch = &download.media_fetch;
    let first_byte = match &download.fetched {
        None => media_fetch.first_byte,
        Some(fetched) if fetched.is_complete() => return None,
        Some(fetched) => fetched.downloaded,
    };
    let next_media_fetch = MediaFetch {
        server_name: media_fetch.server_name.clone(),
        server_track_id: media_fetch.server_track_id.clone(),
        cache_key: media_fetch.cache_key.clone(),
        session: media_fetch.session.clone(),
        first_byte,
        revision: media_fetch.revision,
    };
    let current = player.current().is_some_and(|track| {
        matches!(
            track.source(),
            TrackSource::Server {
                server_name,
                server_track_id,
            } if *server_name == media_fetch.server_name
                && *server_track_id == media_fetch.server_track_id
        )
    });
    Some(if current {
        RemoteCmd::Fetch(next_media_fetch)
    } else {
        RemoteCmd::Prefetch(next_media_fetch)
    })
}

fn listed(level: &mut BrowseLevel, page: Page, catalog_rows: Vec<CatalogRow>) {
    level.paging = if catalog_rows.len() < PAGE_ROWS {
        Paging::Complete
    } else {
        Paging::Next(Page(page.0 + 1))
    };
    if page == Page::default() {
        level.cursor = Cursor::new(catalog_rows.len());
        level.catalog_rows = catalog_rows;
    } else {
        level.catalog_rows.extend(catalog_rows);
        level.cursor = level.cursor.resize(level.catalog_rows.len());
    }
}

pub(crate) fn level(catalog: &mut Catalog) -> &mut BrowseLevel {
    catalog
        .album_level
        .as_mut()
        .unwrap_or(&mut catalog.albums_level)
}

fn online(server_status: &ServerStatus) -> Result<(), Unhandled> {
    match server_status {
        ServerStatus::Online(_) => Ok(()),
        ServerStatus::Connecting | ServerStatus::Offline(_) => Err(Unhandled),
    }
}

fn list(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Cmd {
    let server_name = catalog.server_name.clone();
    let level = level(catalog);
    let (session, page) = match (server_status, level.paging) {
        (ServerStatus::Online(session), Paging::Next(page) | Paging::Loading(page)) => {
            (session.clone(), page)
        }
        (ServerStatus::Online(_), Paging::Complete)
        | (ServerStatus::Connecting | ServerStatus::Offline(_), _) => {
            return Cmd::none();
        }
    };
    level.paging = Paging::Loading(page);
    revisions.list = revisions.issue_effect();
    Effect::Remote(RemoteCmd::List {
        server_name,
        session,
        listing: level.listing.clone(),
        page,
        revision: revisions.list,
    })
    .into()
}

pub(crate) fn show(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Cmd {
    let level = level(catalog);
    match level.paging {
        Paging::Loading(_) => list(catalog, server_status, revisions),
        Paging::Next(_) if level.catalog_rows.is_empty() => {
            list(catalog, server_status, revisions)
        }
        Paging::Next(_) | Paging::Complete => Cmd::none(),
    }
}

pub(crate) fn moved(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Cmd {
    let level = level(catalog);
    match level.paging {
        Paging::Next(_)
            if level.cursor.index() + PAGE_LEAD >= level.catalog_rows.len() =>
        {
            list(catalog, server_status, revisions)
        }
        Paging::Next(_) | Paging::Loading(_) | Paging::Complete => Cmd::none(),
    }
}

pub(crate) fn open(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Result<Cmd, Unhandled> {
    online(server_status)?;
    let level = level(catalog);
    let album_id = match level.cursor.get(&level.catalog_rows).ok_or(Unhandled)? {
        CatalogRow::Album(server_album) => server_album.album_id.clone(),
        CatalogRow::Track(_) => return Err(Unhandled),
    };
    catalog.album_level = Some(BrowseLevel::new(Listing::Album(album_id)));
    Ok(list(catalog, server_status, revisions))
}

pub(crate) fn cycle_sort(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Result<Cmd, Unhandled> {
    online(server_status)?;
    let album_order = match (&catalog.album_level, &catalog.albums_level.listing) {
        (None, Listing::Albums(album_order)) => cycled(*album_order, Direction::Next),
        (Some(_), Listing::Albums(_) | Listing::Album(_))
        | (None, Listing::Album(_)) => {
            return Err(Unhandled);
        }
    };
    catalog.albums_level.listing = Listing::Albums(album_order);
    catalog.albums_level.paging = Paging::Next(Page::default());
    Ok(list(catalog, server_status, revisions))
}

pub(crate) fn level_up(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Result<Cmd, Unhandled> {
    catalog.album_level.take().ok_or(Unhandled)?;
    Ok(show(catalog, server_status, revisions))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect, RemoteCmd},
        domain::{
            driver::DriverName,
            io_error::IoError,
            model::Model,
            player::Player,
            revision::Revision,
            server::{
                Account,
                CacheKey,
                Download,
                Endpoint,
                Fetched,
                MediaFetch,
                RemoteError,
                Server,
                ServerName,
                ServerStatus,
                ServerTrackId,
                Session,
                UserName,
            },
            toast::Toast,
            track::{Track, TrackSource},
        },
        message::{Message, RemoteEvent, Timer},
        update::{
            machine::Unhandled,
            server::{FETCH_RETRY, retry, update},
            server_parts,
            startup::startup_cmd,
        },
    };

    const MIB: u64 = 1024 * 1024;

    fn server(server_status: ServerStatus) -> Server {
        Server {
            account: Account {
                server_name: ServerName::new("home"),
                endpoint: Endpoint::parse("https://music.example.com").unwrap(),
                user_name: UserName::new("ann").unwrap(),
            },
            server_status,
        }
    }

    fn model_with(servers: Vec<Server>, downloads: Vec<Download>) -> Model {
        Model {
            servers,
            downloads,
            ..Model::default()
        }
    }

    fn unreachable(name: &str) -> RemoteError {
        RemoteError::Unreachable {
            server_name: ServerName::new(name),
            source: IoError::Other,
        }
    }

    fn session() -> Session {
        Session::new(
            Endpoint::parse("https://music.example.com").unwrap(),
            "u=ann&t=token&s=salt",
        )
    }

    fn media_fetch(id: &str, first_byte: u64, steps: u64) -> MediaFetch {
        let server_track_id = ServerTrackId::new(id);
        MediaFetch {
            server_name: ServerName::new("home"),
            cache_key: CacheKey::new(
                &ServerName::new("home"),
                &server_track_id,
                "flac",
            ),
            server_track_id,
            session: session(),
            first_byte,
            revision: (0..steps).fold(Revision::default(), |issued, _| issued.next()),
        }
    }

    fn fetched(downloaded: u64) -> Fetched {
        Fetched {
            media_path: PathBuf::from("/cache/home/tr-1.flac.part"),
            downloaded,
            byte_len: 9 * MIB,
        }
    }

    fn download(id: &str, steps: u64, fetched: Option<Fetched>) -> Download {
        Download {
            media_fetch: media_fetch(id, 0, steps),
            fetched,
        }
    }

    fn playing(id: &str) -> Player {
        Player::Loading(Arc::new(Track::from(TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new(id),
        })))
    }

    fn answered(steps: u64, result: Result<Fetched, RemoteError>) -> RemoteEvent {
        RemoteEvent::Fetched {
            revision: media_fetch("", 0, steps).revision,
            result,
        }
    }

    fn online() -> Cmd {
        Cmd::message(Message::Remote(RemoteEvent::Connected {
            server_name: ServerName::new("home"),
            session: session(),
        }))
    }

    #[test]
    fn connected_gives_online() {
        let mut model = model_with(vec![server(ServerStatus::Connecting)], Vec::new());

        let answer = update(
            server_parts(&mut model),
            RemoteEvent::Connected {
                server_name: ServerName::new("home"),
                session: session(),
            },
        );

        assert_eq!(answer, Ok(Cmd::none()));
        assert_eq!(model.servers, vec![server(ServerStatus::Online(session()))]);
    }

    #[test]
    fn unreachable_gives_offline_and_a_toast() {
        let mut model =
            model_with(vec![server(ServerStatus::Online(session()))], Vec::new());

        let answer = update(
            server_parts(&mut model),
            RemoteEvent::Error(unreachable("home")),
        );

        assert_eq!(
            answer,
            Ok(Cmd::message(Message::Toast(Toast::error(
                unreachable("home").to_string()
            ))))
        );
        assert_eq!(
            model.servers,
            vec![server(ServerStatus::Offline(unreachable("home")))]
        );
    }

    #[test]
    fn the_same_error_again_gives_no_second_toast() {
        let mut model = model_with(
            vec![server(ServerStatus::Offline(unreachable("home")))],
            Vec::new(),
        );

        let answer = update(
            server_parts(&mut model),
            RemoteEvent::Error(unreachable("home")),
        );

        assert_eq!(answer, Err(Unhandled));
        assert_eq!(
            model.servers,
            vec![server(ServerStatus::Offline(unreachable("home")))]
        );
    }

    #[rstest]
    #[case::connected(RemoteEvent::Connected {
        server_name: ServerName::new("elsewhere"),
        session: session(),
    })]
    #[case::error(RemoteEvent::Error(unreachable("elsewhere")))]
    fn an_unknown_server_is_refused(#[case] event: RemoteEvent) {
        let mut model = model_with(vec![server(ServerStatus::Connecting)], Vec::new());

        let answer = update(server_parts(&mut model), event);

        assert_eq!(answer, Err(Unhandled));
        assert_eq!(model.servers, vec![server(ServerStatus::Connecting)]);
    }

    #[rstest]
    #[case::incoming(Player::Stopped, ServerStatus::Online(session()), Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1)))))]
    #[case::current(playing("tr-1"), ServerStatus::Online(session()), Cmd::from(Effect::Remote(RemoteCmd::Fetch(media_fetch("tr-1", 4 * MIB, 1)))))]
    #[case::reconnected(Player::Stopped, ServerStatus::Online(Session::new(Endpoint::parse("https://music.example.com").unwrap(), "u=ann&t=fresh&s=pepper")), Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1)))))]
    #[case::offline(Player::Stopped, ServerStatus::Offline(unreachable("home")), Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1)))).then(online()))]
    fn a_chunk_answer_orders_the_next_chunk_and_keeps_a_newer_session(
        #[case] player: Player,
        #[case] server_status: ServerStatus,
        #[case] cmd: Cmd,
    ) {
        let mut model = model_with(
            vec![server(server_status.clone())],
            vec![download("tr-1", 1, None)],
        );
        model.player = player;

        let answer =
            update(server_parts(&mut model), answered(1, Ok(fetched(4 * MIB))));

        assert_eq!(answer, Ok(cmd));
        assert_eq!(
            model.downloads,
            vec![download("tr-1", 1, Some(fetched(4 * MIB)))]
        );
        assert_eq!(model.servers, vec![server(server_status)]);
    }

    #[rstest]
    #[case::last(fetched(8 * MIB), 9 * MIB, None)]
    #[case::same(fetched(4 * MIB), 4 * MIB, Some(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1))))]
    #[case::lower(fetched(4 * MIB), MIB, Some(RemoteCmd::Prefetch(media_fetch("tr-1", MIB, 1))))]
    fn a_chunk_answer_records_its_progress_and_orders_the_next_chunk_from_it(
        #[case] known_fetched: Fetched,
        #[case] downloaded: u64,
        #[case] remote_cmd: Option<RemoteCmd>,
    ) {
        let mut model =
            model_with(Vec::new(), vec![download("tr-1", 1, Some(known_fetched))]);

        let answer = update(
            server_parts(&mut model),
            answered(1, Ok(fetched(downloaded))),
        );

        assert_eq!(
            answer,
            Ok(remote_cmd.map_or_else(Cmd::none, |remote_cmd| {
                Cmd::from(Effect::Remote(remote_cmd))
            }))
        );
        assert_eq!(
            model.downloads,
            vec![download("tr-1", 1, Some(fetched(downloaded)))]
        );
    }

    #[rstest]
    #[case::unknown_revision(answered(2, Ok(fetched(4 * MIB))))]
    #[case::unknown_revision_error(answered(2, Err(unreachable("home"))))]
    fn a_stale_answer_is_refused(#[case] event: RemoteEvent) {
        let downloads = vec![download("tr-1", 1, Some(fetched(4 * MIB)))];
        let mut model = model_with(Vec::new(), downloads.clone());

        let answer = update(server_parts(&mut model), event);

        assert_eq!(answer, Err(Unhandled));
        assert_eq!(model.downloads, downloads);
    }

    #[test]
    fn an_error_answer_retries_the_same_chunk_after_fetch_retry() {
        let mut model = model_with(
            Vec::new(),
            vec![download("tr-1", 1, Some(fetched(4 * MIB)))],
        );
        let revision = media_fetch("tr-1", 0, 1).revision;

        let answer = update(
            server_parts(&mut model),
            answered(1, Err(unreachable("home"))),
        );
        let retried = retry(&model.downloads, &model.player, revision);

        assert_eq!(
            answer,
            Ok(Cmd::from(Effect::After {
                delay: FETCH_RETRY,
                timer: Timer::Fetch(revision),
            })
            .then(Cmd::message(Message::Remote(RemoteEvent::Error(
                unreachable("home")
            )))))
        );
        assert_eq!(
            retried,
            Ok(Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch(
                "tr-1",
                4 * MIB,
                1
            )))))
        );
        assert_eq!(
            retry(
                &model.downloads,
                &model.player,
                media_fetch("", 0, 2).revision
            ),
            Err(Unhandled)
        );
    }

    #[test]
    fn a_restarted_remote_driver_gets_each_unfinished_download_again() {
        let mut model = model_with(
            Vec::new(),
            vec![
                download("tr-1", 1, Some(fetched(4 * MIB))),
                download("tr-2", 2, Some(fetched(9 * MIB))),
                download("tr-3", 3, None),
            ],
        );
        model.player = playing("tr-1");

        let cmd = startup_cmd(&mut model, DriverName::Remote);

        assert_eq!(
            cmd,
            Cmd::from_iter([
                Effect::Remote(RemoteCmd::Fetch(media_fetch("tr-1", 4 * MIB, 1))),
                Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-3", 0, 3))),
            ])
        );
    }
}
