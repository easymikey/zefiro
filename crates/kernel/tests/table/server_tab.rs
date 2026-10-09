use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect, RemoteCmd},
    domain::{
        catalog::{Catalog, CatalogName, Paging},
        cue::Cue,
        cursor::Cursor,
        direction::Direction,
        favorites::{Favorite, Favorites},
        io_error::IoError,
        model::Model,
        overlay::{Field, Overlay, ServerPrompt, ServerQuery, TextEntry},
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
        toast::TOAST_LIFETIME,
        track::{CatalogRow, Track, TrackSource},
    },
    message::{
        BrowseRequest,
        CatalogPage,
        Message,
        QueueRequest,
        RemoteEvent,
        ServerFavorite,
        ServerRequest,
        Timer,
    },
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

pub(crate) fn album_row(album_number: usize) -> CatalogRow {
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
    catalog.albums_level.listing = newest();
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

pub(crate) fn listed(
    model: &Model,
    listing: Listing,
    page: Page,
) -> Result<Cmd, Unhandled> {
    Ok(Effect::Remote(RemoteCmd::List {
        server_name: home(),
        session: session(),
        listing,
        page,
        revision: model.revisions.list,
    })
    .into())
}

pub(crate) fn newest() -> Listing {
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
fn enter_on_an_album_lists_its_tracks_sets_their_stars_and_backspace_closes_it() {
    let mut model = server_model(online(), PAGE_ROWS);
    model.favorites = [server_track("c41d")].into_iter().collect();
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
    let revision = model.revisions.list;
    let starred_tracks = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed(CatalogPage {
            server_name: home(),
            listing,
            page: Page(0),
            catalog_rows: vec![track_row("c41d"), track_row("e7a2")],
            favorites: [server_track("e7a2")].into_iter().collect(),
            revision,
        })),
        Moment::default(),
    );
    assert_eq!(starred_tracks, Ok(Cmd::none()));
    assert_eq!(
        model.favorites.favorite(&server_track("c41d")),
        Favorite::No
    );
    assert_eq!(
        model.favorites.favorite(&server_track("e7a2")),
        Favorite::Yes
    );
    let closed = browse(&mut model, BrowseRequest::LevelUp);
    assert_eq!(closed, Ok(Cmd::none()));
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

#[rstest]
#[case::backspace_after_enter(BrowseRequest::PlaySelected, BrowseRequest::LevelUp)]
#[case::tab_away_and_back(
    BrowseRequest::StepCatalog(Direction::Next),
    BrowseRequest::StepCatalog(Direction::Next)
)]
fn coming_back_to_a_loading_album_list_awaits_its_page(
    #[case] away_request: BrowseRequest,
    #[case] back_request: BrowseRequest,
) {
    let mut model = server_model(online(), PAGE_ROWS);
    drop(browse(&mut model, BrowseRequest::SelectLast));
    drop(browse(&mut model, away_request));

    let answer = browse(&mut model, back_request);

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(model.catalog_name, CatalogName::Server(home()));
    assert_eq!(model.catalogs[0].album_level, None);
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
        Message::Remote(RemoteEvent::Listed(CatalogPage {
            server_name: home(),
            listing: newest(),
            page: Page(1),
            catalog_rows: vec![album_row(PAGE_ROWS)],
            favorites: Favorites::default(),
            revision,
        })),
        Moment::default(),
    );

    assert_eq!(answer, Ok(Cmd::none()));
    let albums = &model.catalogs[0].albums_level;
    assert_eq!(albums.catalog_rows.len(), PAGE_ROWS + 1);
    assert_eq!(albums.paging, Paging::Complete);
    assert_eq!(albums.cursor, Cursor::at(PAGE_ROWS + 1, PAGE_ROWS - 1));
}

#[test]
fn o_cycles_the_album_order_lists_again_and_drops_a_stale_listed() {
    let mut model = server_model(online(), 3);

    let sorted = browse(&mut model, BrowseRequest::CycleSort);

    let listing = Listing::Albums(AlbumOrder::Recent);
    assert_eq!(sorted, listed(&model, listing.clone(), Page(0)));
    assert_eq!(model.catalogs[0].albums_level.listing, listing);
    let catalog_rows = model.catalogs[0].albums_level.catalog_rows.clone();
    let stale = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed(CatalogPage {
            server_name: home(),
            listing,
            page: Page(0),
            catalog_rows: vec![album_row(0)],
            favorites: Favorites::default(),
            revision: Revision::default(),
        })),
        Moment::default(),
    );
    assert_eq!(stale, Err(Unhandled));
    assert_eq!(model.catalogs[0].albums_level.catalog_rows, catalog_rows);
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

fn no_password() -> Message {
    Message::Remote(RemoteEvent::Error(RemoteError::NoPassword {
        server_name: home(),
    }))
}

#[test]
fn a_missing_password_opens_the_server_form_on_the_password_field() {
    let mut model = server_model(ServerStatus::Connecting, 0);

    let answer = update(&mut model, no_password(), Moment::default());

    assert!(answer.is_ok());
    assert_eq!(
        model.workspace.overlay,
        Some(Overlay::AddServer(ServerPrompt {
            origin_server_name: Some(home()),
            link_text_entry: TextEntry {
                input: "https://music.example.com".to_owned(),
                error: None,
            },
            user_text_entry: TextEntry {
                input: "ann".to_owned(),
                error: None,
            },
            field: Field::Password,
            ..ServerPrompt::default()
        }))
    );
    assert_eq!(model.workspace.toasts, Vec::new());
}

#[test]
fn a_missing_password_behind_an_open_overlay_toasts_and_keeps_the_overlay() {
    let mut model = server_model(ServerStatus::Connecting, 0);
    model.workspace.overlay = Some(Overlay::Help);

    let answer = update(&mut model, no_password(), Moment::default());

    assert!(answer.is_ok());
    assert_eq!(model.workspace.overlay, Some(Overlay::Help));
    assert_eq!(
        model
            .workspace
            .toasts
            .iter()
            .map(|toast| toast.title.clone())
            .collect::<Vec<_>>(),
        vec!["No password is saved for home".to_owned()]
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

struct AddRow {
    model: Model,
    connection: Connection,
    servers: Vec<Server>,
    catalogs: Vec<Catalog>,
    toasts: Vec<String>,
    toast_cmd: Cmd,
}

fn add_beside_a_known_server() -> AddRow {
    let model = server_model(ServerStatus::Connecting, 0);
    let added = connection("https://music.example.com", "ann");
    AddRow {
        servers: vec![model.servers[0].clone(), connecting(&added)],
        catalogs: vec![
            model.catalogs[0].clone(),
            Catalog::new(added.account.server_name.clone()),
        ],
        toasts: Vec::new(),
        toast_cmd: Cmd::none(),
        connection: added,
        model,
    }
}

fn add_of_a_known_name() -> AddRow {
    let connection = Connection {
        account: Account {
            server_name: home(),
            endpoint: Endpoint::parse("https://music.example.com/navidrome").unwrap(),
            user_name: UserName::new("bob").unwrap(),
        },
        credential: Credential::Typed(Secret::new("hunter2").unwrap()),
    };
    AddRow {
        model: server_model(online(), 3),
        servers: vec![connecting(&connection)],
        catalogs: vec![Catalog::new(home())],
        toasts: Vec::new(),
        toast_cmd: Cmd::none(),
        connection,
    }
}

fn add_of_an_http_link() -> AddRow {
    let added = connection("http://10.0.0.2:4533", "ann");
    AddRow {
        model: Model::default(),
        servers: vec![connecting(&added)],
        catalogs: vec![Catalog::new(added.account.server_name.clone())],
        toasts: vec![
            "10.0.0.2 uses http://, so its sign-in travels unencrypted".to_owned(),
        ],
        toast_cmd: Cmd::from_iter([
            Effect::Animate(Cue::ToastRaised),
            Effect::After {
                delay: TOAST_LIFETIME,
                timer: Timer::Toast(Revision::default().next()),
            },
        ]),
        connection: added,
    }
}

#[rstest]
#[case::beside_a_known_server(add_beside_a_known_server())]
#[case::of_a_known_name(add_of_a_known_name())]
#[case::of_an_http_link(add_of_an_http_link())]
fn add_gives_connecting_a_connect_and_a_fresh_tab(#[case] row: AddRow) {
    let AddRow {
        mut model,
        connection,
        servers,
        catalogs,
        toasts,
        toast_cmd,
    } = row;
    let accounts = servers
        .iter()
        .map(|server| server.account.clone())
        .collect();

    let answer = request(
        &mut model,
        ServerRequest::Add {
            connection: connection.clone(),
            origin_server_name: None,
        },
    );

    assert_eq!(
        answer,
        connect_and_save(connection, accounts).map(|cmd| cmd.then(toast_cmd))
    );
    assert_eq!(model.servers, servers);
    assert_eq!(model.catalogs, catalogs);
    assert_eq!(
        model
            .workspace
            .toasts
            .iter()
            .map(|toast| toast.title.clone())
            .collect::<Vec<_>>(),
        toasts
    );
}

#[test]
fn reconnect_gives_connecting_and_a_connect_with_the_stored_password() {
    let mut model = offline_model();
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

#[rstest]
#[case::from_the_local_tab(CatalogName::Local)]
#[case::from_its_own_tab(CatalogName::Server(home()))]
fn remove_drops_the_server_and_its_tab_and_leaves_the_local_tab_open(
    #[case] catalog_name: CatalogName,
) {
    let mut model = server_model(online(), 3);
    model.catalog_name = catalog_name;
    let account = model.servers[0].account.clone();

    let answer = request(&mut model, ServerRequest::Remove(home()));

    assert_eq!(answer, removed(account));
    assert_eq!(model.servers, Vec::new());
    assert_eq!(model.catalogs, Vec::new());
    assert_eq!(model.catalog_name, CatalogName::Local);
}

#[test]
fn remove_drops_the_queued_tracks_of_the_server_and_keeps_the_others() {
    let mut model = server_model(online(), 3);
    let other_server_track = TrackSource::Server {
        server_name: ServerName::new("away"),
        server_track_id: ServerTrackId::new("e7a2"),
    };
    let local_track = TrackSource::Local(PathBuf::from("/m/1.flac"));
    model.queue = [
        server_track("c41d"),
        local_track.clone(),
        other_server_track.clone(),
    ]
    .into_iter()
    .map(|track_source| Arc::new(Track::from(track_source)))
    .collect();

    drop(request(&mut model, ServerRequest::Remove(home())));

    let queued: Vec<TrackSource> = model
        .queue
        .iter()
        .map(|queued| queued.source().clone())
        .collect();
    assert_eq!(queued, vec![local_track, other_server_track]);
}

fn server_track(id: &str) -> TrackSource {
    TrackSource::Server {
        server_name: home(),
        server_track_id: ServerTrackId::new(id),
    }
}

pub(crate) fn track_row(id: &str) -> CatalogRow {
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
    Message::Remote(RemoteEvent::Starred(ServerFavorite {
        server_name: home(),
        server_track_id: ServerTrackId::new(id),
        favorite,
    }))
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
    model.catalogs[0].albums_level.server_query = Some(ServerQuery {
        input: "so".to_owned(),
        catalog_rows: Vec::new(),
        revision: Some(current),
    });

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

fn offline_model() -> Model {
    server_model(
        ServerStatus::Offline(RemoteError::Unreachable {
            server_name: home(),
            source: IoError::Other,
        }),
        3,
    )
}

fn local_tab_model() -> Model {
    let mut model = server_model(online(), 3);
    model.catalog_name = CatalogName::Local;
    model
}

fn connecting_album_model() -> Model {
    let mut model = album_model(&["c41d"]);
    model.servers[0].server_status = ServerStatus::Connecting;
    model
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
#[case::level_up_at_the_root(
    server_model(online(), 3),
    Message::Browse(BrowseRequest::LevelUp)
)]
#[case::level_up_in_the_local_tab(
    local_tab_model(),
    Message::Browse(BrowseRequest::LevelUp)
)]
#[case::enter_offline(offline_model(), Message::Browse(BrowseRequest::PlaySelected))]
#[case::favorite_on_an_album_row(
    server_model(online(), 1),
    Message::Browse(BrowseRequest::ToggleFavorite)
)]
#[case::favorite_while_connecting(
    connecting_album_model(),
    Message::Browse(BrowseRequest::ToggleFavorite)
)]
#[case::reconnect_an_unknown_server(
    server_model(ServerStatus::Connecting, 3),
    Message::Server(ServerRequest::Reconnect(ServerName::new("gone")))
)]
#[case::remove_an_unknown_server(
    server_model(ServerStatus::Connecting, 3),
    Message::Server(ServerRequest::Remove(ServerName::new("gone")))
)]
#[case::dequeue(
    local_playlist_behind_a_server_tab(),
    Message::Queue(QueueRequest::Dequeue)
)]
#[case::move_up(
    local_playlist_behind_a_server_tab(),
    Message::Queue(QueueRequest::Move(Direction::Previous))
)]
#[case::move_down(
    local_playlist_behind_a_server_tab(),
    Message::Queue(QueueRequest::Move(Direction::Next))
)]
#[case::save_playlist(
    local_playlist_behind_a_server_tab(),
    Message::Browse(BrowseRequest::SavePlaylist(PlaylistFileName::new("mix").unwrap()))
)]
#[case::trash(
    local_playlist_behind_a_server_tab(),
    Message::Browse(BrowseRequest::Trash(TrackSource::Local(PathBuf::from(
        "/m/1.flac"
    ))))
)]
#[case::rescan(
    local_playlist_behind_a_server_tab(),
    Message::Browse(BrowseRequest::Rescan)
)]
fn a_request_the_server_tab_cannot_take_is_refused_and_changes_nothing(
    #[case] mut model: Model,
    #[case] message: Message,
) {
    let before = model.clone();

    assert_eq!(
        update(&mut model, message, Moment::default()),
        Err(Unhandled)
    );
    assert_eq!(model, before);
}
