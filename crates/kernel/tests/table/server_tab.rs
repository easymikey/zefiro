use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect, RemoteCmd},
    domain::{
        catalog::{Catalog, CatalogName, Paging},
        cue::Cue,
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        favorites::{Favorite, Favorites},
        io_error::IoError,
        model::Model,
        overlay::{Overlay, ServerQuery},
        playlist::PlaylistFileName,
        revision::Revision,
        server::{
            Account,
            AlbumId,
            AlbumOrder,
            ApiCode,
            Connection,
            Credential,
            Endpoint,
            Listing,
            PAGE_ROWS,
            Page,
            RemoteError,
            Secret,
            Server,
            ServerAlbum,
            ServerName,
            ServerStatus,
            ServerTrackId,
            Session,
            UserName,
        },
        time::Moment,
        track::{CatalogRow, Track, TrackSource},
    },
    message::{BrowseRequest, Message, QueueRequest, RemoteEvent, ServerRequest},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    router::{moon_library_scanned, queued},
    update::update,
};

pub(crate) fn home() -> ServerName {
    ServerName::new("home")
}

pub(crate) fn session() -> Session {
    Session::new(
        Endpoint::parse("https://music.example.com").unwrap(),
        "u=ann",
    )
}

pub(crate) fn online() -> ServerStatus {
    ServerStatus::Online(session())
}

fn album_row(album_number: usize) -> CatalogRow {
    CatalogRow::Album(ServerAlbum {
        album_id: AlbumId::new(&format!("al-{album_number}")),
        title: Arc::from("Title"),
        artist: Arc::from("Artist"),
        year: None,
        track_count: 1,
        duration: Duration::from_secs(60),
    })
}

pub(crate) fn server_model(server_status: ServerStatus, rows: usize) -> Model {
    let server = Server {
        account: Account {
            server_name: home(),
            endpoint: Endpoint::parse("https://music.example.com").unwrap(),
            user_name: UserName::new("ann").unwrap(),
        },
        server_status,
    };
    let mut catalog = Catalog::new(home());
    catalog.albums_level.catalog_rows = (0..rows).map(album_row).collect();
    catalog.albums_level.cursor = Cursor::new(rows);
    catalog.albums_level.paging = Paging::Next(Page(usize::from(rows > 0)));
    Model {
        servers: vec![server],
        catalog_name: CatalogName::Server(home()),
        catalogs: vec![catalog],
        ..Model::default()
    }
}

pub(crate) fn browse(
    model: &mut Model,
    request: BrowseRequest,
) -> Result<Cmd, Unhandled> {
    update(model, Message::Browse(request), Moment::default())
}

fn listed(model: &Model, listing: Listing, page: Page) -> Result<Cmd, Unhandled> {
    Ok(Effect::Remote(RemoteCmd::List {
        server_name: home(),
        session: session(),
        listing,
        page,
        revision: model.revisions.list,
    })
    .into())
}

fn newest() -> Listing {
    Listing::Albums(AlbumOrder::Newest)
}

#[test]
fn tab_goes_from_local_to_the_server_and_back() {
    let mut model = server_model(online(), 0);
    model.catalog_name = CatalogName::Local;

    let answer = browse(&mut model, BrowseRequest::StepCatalog(Direction::Next));

    assert_eq!(answer, listed(&model, newest(), Page(0)));
    assert_eq!(model.catalog_name, CatalogName::Server(home()));
    assert_eq!(
        model.catalogs[0].albums_level.paging,
        Paging::Loading(Page(0))
    );
    assert_eq!(
        browse(&mut model, BrowseRequest::StepCatalog(Direction::Next)),
        Ok(Cmd::none())
    );
    assert_eq!(model.catalog_name, CatalogName::Local);
}

#[test]
fn an_added_server_gets_its_own_tab() {
    let mut model = Model::default();
    let connection = Connection {
        account: Account {
            server_name: home(),
            endpoint: Endpoint::parse("https://music.example.com").unwrap(),
            user_name: UserName::new("ann").unwrap(),
        },
        credential: Credential::Typed(Secret::new("hunter2").unwrap()),
    };
    let added = update(
        &mut model,
        Message::Server(ServerRequest::Add {
            connection,
            origin_server_name: None,
        }),
        Moment::default(),
    );
    assert!(added.is_ok());

    let answer = browse(&mut model, BrowseRequest::StepCatalog(Direction::Next));

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(model.catalog_name, CatalogName::Server(home()));
    assert_eq!(model.catalogs, vec![Catalog::new(home())]);
}

#[test]
fn adding_a_known_server_again_starts_its_tab_afresh() {
    let mut model = server_model(online(), 3);
    let connection = Connection {
        account: model.servers[0].account.clone(),
        credential: Credential::Typed(Secret::new("hunter2").unwrap()),
    };

    let added = update(
        &mut model,
        Message::Server(ServerRequest::Add {
            connection,
            origin_server_name: None,
        }),
        Moment::default(),
    );
    assert!(added.is_ok());

    assert_eq!(model.catalogs, vec![Catalog::new(home())]);
}

#[test]
fn backspace_at_the_root_is_refused() {
    let mut model = server_model(online(), 3);

    assert_eq!(browse(&mut model, BrowseRequest::LevelUp), Err(Unhandled));
    model.catalog_name = CatalogName::Local;
    assert_eq!(browse(&mut model, BrowseRequest::LevelUp), Err(Unhandled));
}

#[test]
fn enter_on_an_album_lists_its_tracks_and_backspace_closes_it() {
    let mut model = server_model(online(), 3);
    drop(browse(&mut model, BrowseRequest::CursorBy { rows: 1 }));

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    let listing = Listing::Album(AlbumId::new("al-1"));
    assert_eq!(answer, listed(&model, listing.clone(), Page(0)));
    assert_eq!(
        model.catalogs[0]
            .album_level
            .as_ref()
            .map(|level| &level.listing),
        Some(&listing)
    );
    let closed = browse(&mut model, BrowseRequest::LevelUp);
    assert_eq!(closed, listed(&model, newest(), Page(1)));
    assert_eq!(model.catalogs[0].album_level, None);
}

#[test]
fn a_cursor_into_the_last_rows_asks_the_next_page_once() {
    let mut model = server_model(online(), PAGE_ROWS);

    let answer = browse(&mut model, BrowseRequest::CursorBy { rows: 180 });

    assert_eq!(answer, listed(&model, newest(), Page(1)));
    assert_eq!(
        model.catalogs[0].albums_level.paging,
        Paging::Loading(Page(1))
    );
    assert_eq!(
        browse(&mut model, BrowseRequest::CursorBy { rows: 1 }),
        Ok(Cmd::none())
    );
}

#[test]
fn backspace_after_enter_during_a_page_load_asks_that_page_again() {
    let mut model = server_model(online(), PAGE_ROWS);
    drop(browse(&mut model, BrowseRequest::SelectLast));
    drop(browse(&mut model, BrowseRequest::PlaySelected));

    let answer = browse(&mut model, BrowseRequest::LevelUp);

    assert_eq!(answer, listed(&model, newest(), Page(1)));
    assert_eq!(model.catalogs[0].album_level, None);
    assert_eq!(
        model.catalogs[0].albums_level.paging,
        Paging::Loading(Page(1))
    );
}

#[test]
fn tab_away_from_a_loading_server_and_back_asks_the_page_again() {
    let mut model = server_model(online(), PAGE_ROWS);
    drop(browse(&mut model, BrowseRequest::SelectLast));
    drop(browse(
        &mut model,
        BrowseRequest::StepCatalog(Direction::Next),
    ));

    let answer = browse(&mut model, BrowseRequest::StepCatalog(Direction::Next));

    assert_eq!(model.catalog_name, CatalogName::Server(home()));
    assert_eq!(answer, listed(&model, newest(), Page(1)));
    assert_eq!(
        model.catalogs[0].albums_level.paging,
        Paging::Loading(Page(1))
    );
}

#[test]
fn a_cursor_above_the_last_rows_asks_nothing() {
    let mut model = server_model(online(), PAGE_ROWS);

    let answer = browse(&mut model, BrowseRequest::CursorBy { rows: 179 });

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].albums_level.paging, Paging::Next(Page(1)));
}

#[test]
fn listed_appends_a_later_page_and_completes_on_a_short_one() {
    let mut model = server_model(online(), PAGE_ROWS);
    drop(browse(&mut model, BrowseRequest::SelectLast));
    let revision = model.revisions.list;

    let answer = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed {
            server_name: home(),
            listing: newest(),
            page: Page(1),
            catalog_rows: vec![album_row(PAGE_ROWS)],
            favorites: Favorites::default(),
            revision,
        }),
        Moment::default(),
    );

    assert_eq!(answer, Ok(Cmd::none()));
    let albums = &model.catalogs[0].albums_level;
    assert_eq!(albums.catalog_rows.len(), PAGE_ROWS + 1);
    assert_eq!(albums.paging, Paging::Complete);
    assert_eq!(albums.cursor, Cursor::at(PAGE_ROWS + 1, PAGE_ROWS - 1));
}

#[test]
fn a_stale_listed_is_dropped() {
    let mut model = server_model(online(), 0);
    drop(browse(&mut model, BrowseRequest::CycleSort));

    let answer = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed {
            server_name: home(),
            listing: Listing::Albums(AlbumOrder::Recent),
            page: Page(0),
            catalog_rows: vec![album_row(0)],
            favorites: Favorites::default(),
            revision: Revision::default(),
        }),
        Moment::default(),
    );

    assert_eq!(answer, Err(Unhandled));
    assert!(model.catalogs[0].albums_level.catalog_rows.is_empty());
}

#[test]
fn o_cycles_the_album_order_and_lists_again() {
    let mut model = server_model(online(), 3);

    let answer = browse(&mut model, BrowseRequest::CycleSort);

    let listing = Listing::Albums(AlbumOrder::Recent);
    assert_eq!(answer, listed(&model, listing.clone(), Page(0)));
    assert_eq!(model.catalogs[0].albums_level.listing, listing);
}

#[test]
fn an_offline_tab_refuses_enter() {
    let mut model = server_model(
        ServerStatus::Offline(RemoteError::Unreachable {
            server_name: home(),
            source: IoError::Other,
        }),
        3,
    );

    assert_eq!(
        browse(&mut model, BrowseRequest::PlaySelected),
        Err(Unhandled)
    );
    assert_eq!(model.catalogs[0].album_level, None);
}

#[test]
fn a_connecting_tab_lists_the_first_page_when_its_server_goes_online() {
    let mut model = server_model(ServerStatus::Connecting, 0);
    model.catalog_name = CatalogName::Local;
    let shown = browse(&mut model, BrowseRequest::StepCatalog(Direction::Next));
    assert_eq!(shown, Ok(Cmd::none()));

    let answer = update(
        &mut model,
        Message::Remote(RemoteEvent::Connected {
            server_name: home(),
            session: session(),
        }),
        Moment::default(),
    );

    assert_eq!(answer, listed(&model, newest(), Page(0)));
    assert_eq!(
        model.catalogs[0].albums_level.paging,
        Paging::Loading(Page(0))
    );
}

fn connection(link: &str, user: &str) -> Connection {
    let endpoint = Endpoint::parse(link).unwrap();
    Connection {
        account: Account {
            server_name: ServerName::new(endpoint.host()),
            endpoint,
            user_name: UserName::new(user).unwrap(),
        },
        credential: Credential::Typed(Secret::new("hunter2").unwrap()),
    }
}

fn connecting(connection: &Connection) -> Server {
    Server {
        account: connection.account.clone(),
        server_status: ServerStatus::Connecting,
    }
}

fn connect(connection: Connection) -> Result<Cmd, Unhandled> {
    Ok(Cmd::from(Effect::Remote(RemoteCmd::Connect(connection))))
}

fn saved(accounts: Vec<Account>) -> Cmd {
    Cmd::from(Effect::Config(ConfigCmd::Save(ConfigPatch {
        accounts: Some(accounts),
        ..ConfigPatch::default()
    })))
}

fn connect_and_save(
    connection: Connection,
    accounts: Vec<Account>,
) -> Result<Cmd, Unhandled> {
    connect(connection).map(|cmd| cmd.then(saved(accounts)))
}

fn removed(account: Account) -> Result<Cmd, Unhandled> {
    Ok(saved(Vec::new()).then(Cmd::from(Effect::Remote(RemoteCmd::Forget(account)))))
}

fn request(model: &mut Model, server_request: ServerRequest) -> Result<Cmd, Unhandled> {
    update(model, Message::Server(server_request), Moment::default())
}

#[test]
fn add_gives_connecting_and_a_connect() {
    let added = connection("https://music.example.com", "ann");
    let mut model = server_model(ServerStatus::Connecting, 0);
    let known = model.servers[0].clone();

    let answer = request(
        &mut model,
        ServerRequest::Add {
            connection: added.clone(),
            origin_server_name: None,
        },
    );

    assert_eq!(
        answer,
        connect_and_save(
            added.clone(),
            vec![known.account.clone(), added.account.clone()]
        )
    );
    assert_eq!(model.servers, vec![known, connecting(&added)]);
}

#[test]
fn add_of_a_known_host_replaces_it_and_connects_again() {
    let known = connection("https://music.example.com", "ann");
    let added = connection("https://music.example.com/navidrome", "bob");
    let mut model = Model {
        servers: vec![Server {
            account: known.account,
            server_status: online(),
        }],
        ..Model::default()
    };

    let answer = request(
        &mut model,
        ServerRequest::Add {
            connection: added.clone(),
            origin_server_name: None,
        },
    );

    assert_eq!(
        answer,
        connect_and_save(added.clone(), vec![added.account.clone()])
    );
    assert_eq!(model.servers, vec![connecting(&added)]);
}

#[test]
fn add_of_an_http_link_warns_that_the_sign_in_is_unencrypted() {
    let added = connection("http://10.0.0.2:4533", "ann");
    let mut model = Model::default();

    let answer = request(
        &mut model,
        ServerRequest::Add {
            connection: added.clone(),
            origin_server_name: None,
        },
    );

    assert!(answer.is_ok());
    assert_eq!(model.servers, vec![connecting(&added)]);
    assert_eq!(
        model
            .workspace
            .toasts
            .iter()
            .map(|toast| toast.title.clone())
            .collect::<Vec<_>>(),
        vec!["10.0.0.2 uses http://, so its sign-in travels unencrypted".to_owned()]
    );
}

#[test]
fn reconnect_gives_connecting_and_a_connect_with_the_stored_password() {
    let mut model = server_model(
        ServerStatus::Offline(RemoteError::Unreachable {
            server_name: home(),
            source: IoError::Other,
        }),
        0,
    );
    let account = model.servers[0].account.clone();

    let answer = request(&mut model, ServerRequest::Reconnect(home()));

    assert_eq!(
        answer,
        connect(Connection {
            account,
            credential: Credential::Stored,
        })
    );
    assert_eq!(model.servers[0].server_status, ServerStatus::Connecting);
}

#[test]
fn remove_drops_the_server_and_its_tab() {
    let mut model = server_model(online(), 3);
    model.catalog_name = CatalogName::Local;
    let account = model.servers[0].account.clone();

    let answer = request(&mut model, ServerRequest::Remove(home()));

    assert_eq!(answer, removed(account));
    assert_eq!(model.servers, Vec::new());
    assert_eq!(model.catalogs, Vec::new());
}

#[test]
fn remove_of_the_server_whose_tab_is_open_returns_to_the_local_tab() {
    let mut model = server_model(online(), 3);
    let account = model.servers[0].account.clone();

    let answer = request(&mut model, ServerRequest::Remove(home()));

    assert_eq!(answer, removed(account));
    assert_eq!(model.catalog_name, CatalogName::Local);
    assert_eq!(model.catalogs, Vec::new());
}

#[rstest]
#[case::reconnect(ServerRequest::Reconnect(ServerName::new("gone")))]
#[case::remove(ServerRequest::Remove(ServerName::new("gone")))]
fn a_request_for_an_unknown_server_is_refused_and_changes_nothing(
    #[case] server_request: ServerRequest,
) {
    let mut model = server_model(ServerStatus::Connecting, 3);
    let servers = model.servers.clone();
    let catalogs = model.catalogs.clone();

    assert_eq!(request(&mut model, server_request), Err(Unhandled));
    assert_eq!(model.servers, servers);
    assert_eq!(model.catalogs, catalogs);
}

fn server_track(id: &str) -> TrackSource {
    TrackSource::Server {
        server_name: home(),
        server_track_id: ServerTrackId::new(id),
    }
}

fn track_row(id: &str) -> CatalogRow {
    CatalogRow::Track(Arc::new(Track::from(server_track(id))))
}

fn album_model(ids: &[&str]) -> Model {
    let mut model = server_model(online(), 0);
    let albums = &mut model.catalogs[0].albums_level;
    albums.catalog_rows = ids.iter().copied().map(track_row).collect();
    albums.cursor = Cursor::new(ids.len());
    model
}

fn starred(id: &str, favorite: Favorite) -> Message {
    Message::Remote(RemoteEvent::Starred {
        server_name: home(),
        server_track_id: ServerTrackId::new(id),
        favorite,
    })
}

#[test]
fn f_on_a_server_track_flips_its_star_at_once_and_orders_the_star() {
    let mut model = album_model(&["c41d"]);

    let answer = browse(&mut model, BrowseRequest::ToggleFavorite);

    assert_eq!(
        answer,
        Ok(Cmd::from_iter([
            Effect::Remote(RemoteCmd::Star {
                server_name: home(),
                session: session(),
                server_track_id: ServerTrackId::new("c41d"),
                favorite: Favorite::Yes,
            }),
            Effect::Animate(Cue::FavoriteToggled),
        ]))
    );
    assert_eq!(
        model.favorites.favorite(&server_track("c41d")),
        Favorite::Yes
    );
}

#[test]
fn f_on_an_album_row_or_an_offline_tab_is_refused() {
    let mut albums = server_model(online(), 1);
    let mut offline = album_model(&["c41d"]);
    offline.servers[0].server_status = ServerStatus::Connecting;

    assert_eq!(
        browse(&mut albums, BrowseRequest::ToggleFavorite),
        Err(Unhandled)
    );
    assert_eq!(
        browse(&mut offline, BrowseRequest::ToggleFavorite),
        Err(Unhandled)
    );
    assert_eq!(offline.favorites, Favorites::default());
}

#[test]
fn a_starred_answer_sets_the_star_the_server_holds_and_a_failed_star_toasts() {
    let mut model = album_model(&["c41d"]);
    drop(browse(&mut model, BrowseRequest::ToggleFavorite));
    let remote_error = RemoteError::Api {
        server_name: home(),
        api_code: ApiCode(70),
    };

    let reverted = update(&mut model, starred("c41d", Favorite::No), Moment::default());
    let again = update(&mut model, starred("c41d", Favorite::No), Moment::default());
    let failed = update(
        &mut model,
        Message::Remote(RemoteEvent::Error(remote_error.clone())),
        Moment::default(),
    );

    assert_eq!(reverted, Ok(Cmd::none()));
    assert_eq!(
        model.favorites.favorite(&server_track("c41d")),
        Favorite::No
    );
    assert_eq!(again, Err(Unhandled));
    assert!(failed.is_ok());
    assert_eq!(
        model
            .workspace
            .toasts
            .iter()
            .map(|toast| toast.title.clone())
            .collect::<Vec<_>>(),
        vec![remote_error.to_string()]
    );
}

#[test]
fn a_listed_album_sets_each_tracks_star_from_the_server() {
    let mut model = server_model(online(), 1);
    model.favorites = [server_track("c41d")].into_iter().collect();
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    let revision = model.revisions.list;

    let answer = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed {
            server_name: home(),
            listing: Listing::Album(AlbumId::new("al-0")),
            page: Page(0),
            catalog_rows: vec![track_row("c41d"), track_row("e7a2")],
            favorites: [server_track("e7a2")].into_iter().collect(),
            revision,
        }),
        Moment::default(),
    );

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(
        model.favorites.favorite(&server_track("c41d")),
        Favorite::No
    );
    assert_eq!(
        model.favorites.favorite(&server_track("e7a2")),
        Favorite::Yes
    );
}

fn found(model: &mut Model, revision: Revision) -> Result<Cmd, Unhandled> {
    update(
        model,
        Message::Remote(RemoteEvent::Found {
            server_name: home(),
            result: Ok((
                vec![album_row(0), track_row("c41d"), track_row("e7a2")],
                [server_track("e7a2")].into_iter().collect(),
            )),
            revision,
        }),
        Moment::default(),
    )
}

#[test]
fn a_found_track_shows_the_star_the_server_holds_only_for_the_current_revision() {
    let current = Revision::default().next();
    let mut model = server_model(online(), 0);
    model.favorites = [server_track("c41d")].into_iter().collect();
    model.workspace.overlay = Some(Overlay::ServerSearch(CursorOver::new(
        ServerQuery {
            server_name: home(),
            input: "so".to_owned(),
            catalog_rows: Vec::new(),
            revision: Some(current),
        },
        0,
    )));

    let stale = found(&mut model, current.next());
    assert_eq!(stale, Err(Unhandled));
    assert_eq!(
        model.favorites.favorite(&server_track("e7a2")),
        Favorite::No
    );

    let answer = found(&mut model, current);

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(
        model.favorites.favorite(&server_track("c41d")),
        Favorite::No
    );
    assert_eq!(
        model.favorites.favorite(&server_track("e7a2")),
        Favorite::Yes
    );
}

fn local_playlist_behind_a_server_tab() -> Model {
    let tab = server_model(online(), 3);
    let mut model = queued(moon_library_scanned(), &[0, 1, 2]);
    model.workspace.browse.cursor = Cursor::at(3, 1);
    model.servers = tab.servers;
    model.catalogs = tab.catalogs;
    model.catalog_name = tab.catalog_name;
    model
}

#[rstest]
#[case::dequeue(Message::Queue(QueueRequest::Dequeue))]
#[case::move_up(Message::Queue(QueueRequest::Move(Direction::Previous)))]
#[case::move_down(Message::Queue(QueueRequest::Move(Direction::Next)))]
#[case::save_playlist(Message::Browse(BrowseRequest::SavePlaylist(
    PlaylistFileName::new("mix").unwrap()
)))]
#[case::trash(Message::Browse(BrowseRequest::Trash(TrackSource::Local(
    PathBuf::from("/m/1.flac")
))))]
#[case::rescan(Message::Browse(BrowseRequest::Rescan))]
fn a_server_tab_refuses_the_local_playlist_keys_and_changes_nothing(
    #[case] message: Message,
) {
    let mut model = local_playlist_behind_a_server_tab();
    let before = model.clone();

    assert_eq!(
        update(&mut model, message, Moment::default()),
        Err(Unhandled)
    );
    assert_eq!(model, before);
}
