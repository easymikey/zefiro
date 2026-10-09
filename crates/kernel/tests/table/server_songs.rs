use std::{sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        catalog::{BrowseLevel, Catalog, CatalogName, Paging},
        cursor::Cursor,
        direction::Direction,
        favorites::Favorites,
        index::ViewIndex,
        model::Model,
        playlist::{PlayOrder, PlaylistSource},
        server::{AlbumId, Listing, PAGE_ROWS, Page, PlaylistId, ServerPlaylist},
        time::Moment,
        track::CatalogRow,
    },
    message::{AudioEvent, BrowseRequest, CatalogPage, Message, RemoteEvent},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::{
    support::{router::shuffled, update::update},
    table::server_tab::{
        album_row,
        browse,
        home,
        listed,
        newest,
        online,
        server_model,
        session,
        track_row,
    },
};

pub(crate) fn songs_model(rows: usize) -> Model {
    let mut model = server_model(online(), 0);
    let mut catalog = Catalog::new(home());
    catalog.albums_level.catalog_rows = (0..rows)
        .map(|number| track_row(&format!("s-{number}")))
        .collect();
    catalog.albums_level.cursor = Cursor::new(rows);
    catalog.albums_level.paging = Paging::Next(Page(usize::from(rows > 0)));
    model.catalogs = vec![catalog];
    model
}

#[test]
fn a_server_tab_opens_on_songs_and_v_cycles_to_newest_albums_and_back() {
    let mut model = songs_model(0);
    model.catalog_name = CatalogName::Local;

    let answer = browse(&mut model, BrowseRequest::StepCatalog(Direction::Next));

    assert_eq!(answer, listed(&model, Listing::Songs, Page(0)));
    let albums = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(albums, Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].albums_level.listing, newest());
    let revision = model.revisions.list;
    let songs_page = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed(CatalogPage {
            server_name: home(),
            listing: Listing::Songs,
            page: Page(0),
            catalog_rows: vec![track_row("s-0")],
            favorites: Favorites::default(),
            revision,
        })),
        Moment::default(),
    );
    assert_eq!(songs_page, listed(&model, newest(), Page(0)));
    let playlists = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(playlists, Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].albums_level.listing, Listing::Playlists);
    let songs = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(songs, Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].albums_level.listing, Listing::Songs);
}

#[test]
fn enter_on_a_song_plays_the_loaded_songs_and_near_the_end_takes_in_the_next_page() {
    let mut model = songs_model(3);
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    assert_eq!(model.playlist_source, PlaylistSource::Songs(home()));

    drop(update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    ));

    let revision = model.revisions.list;
    let taken = update(
        &mut model,
        Message::Remote(RemoteEvent::Listed(CatalogPage {
            server_name: home(),
            listing: Listing::Songs,
            page: Page(1),
            catalog_rows: vec![track_row("s-3"), track_row("s-4")],
            favorites: Favorites::default(),
            revision,
        })),
        Moment::default(),
    );
    assert_eq!(taken, Ok(Cmd::none()));
    assert_eq!(model.playlist.tracks.len(), 5);
    assert_eq!(model.playlist.cursor.index(), 0);
}

fn requested(model: &Model, listing: Listing, page: Page) -> Effect {
    Effect::Remote(RemoteCmd::List {
        server_name: home(),
        session: session(),
        listing,
        page,
        revision: model.revisions.list,
    })
}

fn loaded(model: &mut Model) -> Vec<Effect> {
    let answer = update(
        model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    );
    let (effects, _messages) = answer.unwrap().into_parts();
    effects
}

fn answered(
    model: &mut Model,
    listing: Listing,
    catalog_rows: Vec<CatalogRow>,
) -> Result<Cmd, Unhandled> {
    let page = Page(usize::from(listing == Listing::Songs));
    let revision = model.revisions.list;
    update(
        model,
        Message::Remote(RemoteEvent::Listed(CatalogPage {
            server_name: home(),
            listing,
            page,
            catalog_rows,
            favorites: Favorites::default(),
            revision,
        })),
        Moment::default(),
    )
}

fn taken(model: &mut Model, catalog_rows: Vec<CatalogRow>) -> Result<Cmd, Unhandled> {
    answered(model, Listing::Songs, catalog_rows)
}

#[test]
fn a_song_plays_on_through_v_and_near_the_end_the_next_songs_page_is_taken_in() {
    let mut model = songs_model(3);
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    let albums = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(albums, Ok(requested(&model, newest(), Page(0)).into()));
    assert!(!loaded(&mut model).contains(&requested(&model, Listing::Songs, Page(1))));

    let albums_page = answered(&mut model, newest(), vec![album_row(0), album_row(1)]);

    assert_eq!(
        albums_page,
        Ok(requested(&model, Listing::Songs, Page(1)).into())
    );
    assert_eq!(model.catalogs[0].albums_level.catalog_rows.len(), 2);
    let answer = taken(&mut model, vec![track_row("s-3"), track_row("s-4")]);
    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(model.playlist.tracks.len(), 5);
    assert_eq!(model.catalogs[0].albums_level.listing, newest());
    assert_eq!(
        browse(&mut model, BrowseRequest::CycleView),
        Ok(requested(&model, Listing::Playlists, Page(0)).into())
    );
    assert_eq!(
        browse(&mut model, BrowseRequest::CycleView),
        Ok(Cmd::none())
    );
    assert_eq!(model.catalogs[0].albums_level.catalog_rows.len(), 5);
}

#[test]
fn a_songs_page_in_flight_extends_the_playlist_before_the_albums_page_is_asked() {
    let mut model = songs_model(3);
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    assert!(loaded(&mut model).contains(&requested(&model, Listing::Songs, Page(1))));
    let albums = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(albums, Ok(Cmd::none()));

    let answer = taken(&mut model, vec![track_row("s-3"), track_row("s-4")]);

    assert_eq!(answer, Ok(requested(&model, newest(), Page(0)).into()));
    assert_eq!(model.playlist.tracks.len(), 5);
    let albums_page = answered(&mut model, newest(), vec![album_row(0)]);
    assert_eq!(albums_page, Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].albums_level.catalog_rows.len(), 1);
}

#[test]
fn the_albums_view_keeps_its_order_across_v() {
    let mut model = songs_model(0);
    drop(browse(&mut model, BrowseRequest::CycleView));
    drop(browse(&mut model, BrowseRequest::CycleSort));
    let order = model.catalogs[0].albums_level.listing.clone();

    drop(browse(&mut model, BrowseRequest::CycleView));
    drop(browse(&mut model, BrowseRequest::CycleView));
    drop(browse(&mut model, BrowseRequest::CycleView));

    assert_ne!(order, newest());
    assert_eq!(model.catalogs[0].albums_level.listing, order);
}

fn shuffled_songs(played: usize) -> Model {
    let mut model = songs_model(30);
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    let order = (1..=played).chain([0]).chain(played + 1..30);
    model.playlist.play_order =
        PlayOrder::Shuffled(order.map(ViewIndex::new).collect());
    model
}

#[test]
fn under_shuffle_the_next_songs_page_loads_near_the_end_of_the_play_order() {
    let mut model = shuffled_songs(25);

    assert!(loaded(&mut model).contains(&requested(&model, Listing::Songs, Page(1))));
}

#[test]
fn under_shuffle_a_page_taken_in_keeps_the_played_part_of_the_order() {
    let mut model = shuffled_songs(25);
    drop(loaded(&mut model));
    drop(taken(
        &mut model,
        vec![track_row("s-30"), track_row("s-31")],
    ));

    drop(update(
        &mut model,
        shuffled((0..32).rev().collect()),
        Moment::default(),
    ));

    let kept = (1..=25).chain([0]).chain((26..32).rev());
    assert_eq!(
        model.playlist.play_order,
        PlayOrder::Shuffled(kept.map(ViewIndex::new).collect())
    );
}

fn playlist_row(playlist_number: usize) -> CatalogRow {
    CatalogRow::Playlist(ServerPlaylist {
        playlist_id: PlaylistId::new(&format!("pl-{playlist_number}")),
        name: Arc::from("Late Night"),
        track_count: 12,
        duration: Duration::from_secs(3_120),
    })
}

fn playlists_model() -> Model {
    let mut model = songs_model(0);
    let level = &mut model.catalogs[0].albums_level;
    level.listing = Listing::Playlists;
    level.catalog_rows = vec![playlist_row(0), playlist_row(1)];
    level.cursor = Cursor::new(2);
    level.paging = Paging::Complete;
    model
}

#[test]
fn v_cycles_songs_albums_and_playlists_and_back_to_songs() {
    let mut model = songs_model(1);

    let albums = browse(&mut model, BrowseRequest::CycleView);

    assert_eq!(albums, listed(&model, newest(), Page(0)));
    let albums_page = answered(&mut model, newest(), vec![album_row(0)]);
    assert_eq!(albums_page, Ok(Cmd::none()));
    let playlists = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(playlists, listed(&model, Listing::Playlists, Page(0)));
    assert_eq!(model.catalogs[0].albums_level.listing, Listing::Playlists);
    let songs = browse(&mut model, BrowseRequest::CycleView);
    assert_eq!(songs, Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].albums_level.listing, Listing::Songs);
    assert_eq!(model.catalogs[0].albums_level.catalog_rows.len(), 1);
}

#[rstest]
#[case::album(
    server_model(online(), PAGE_ROWS),
    Listing::Album(AlbumId::new("al-1")),
    newest()
)]
#[case::playlist(
    playlists_model(),
    Listing::Playlist(PlaylistId::new("pl-1")),
    Listing::Playlists
)]
fn enter_on_an_album_or_a_playlist_lists_all_its_tracks_at_once_and_backspace_goes_back(
    #[case] mut model: Model,
    #[case] listing: Listing,
    #[case] parent_listing: Listing,
) {
    drop(browse(&mut model, BrowseRequest::CursorBy { rows: 1 }));

    let opened = browse(&mut model, BrowseRequest::PlaySelected);

    assert_eq!(opened, listed(&model, listing.clone(), Page(0)));
    let tracks = (0..PAGE_ROWS)
        .map(|number| track_row(&format!("t-{number}")))
        .collect();
    let page = answered(&mut model, listing, tracks);
    assert_eq!(page, Ok(Cmd::none()));
    let level = model.catalogs[0].album_level.as_ref();
    assert_eq!(level.map(|level| level.paging), Some(Paging::Complete));
    assert_eq!(level.map(|level| level.catalog_rows.len()), Some(PAGE_ROWS));
    assert_eq!(browse(&mut model, BrowseRequest::LevelUp), Ok(Cmd::none()));
    assert_eq!(model.catalogs[0].album_level, None);
    assert_eq!(model.catalogs[0].albums_level.listing, parent_listing);
}

#[rstest]
#[case::album(server_model(online(), PAGE_ROWS))]
#[case::playlist(playlists_model())]
fn v_inside_an_opened_album_or_playlist_closes_it_and_moves_to_the_next_view(
    #[case] mut model: Model,
) {
    drop(browse(&mut model, BrowseRequest::CursorBy { rows: 1 }));
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    assert_ne!(model.catalogs[0].album_level, None);
    let [next_level, _] = &model.catalogs[0].browse_levels;
    let next_listing = next_level.listing.clone();

    let next = browse(&mut model, BrowseRequest::CycleView);

    assert_ne!(next, Err(Unhandled));
    assert_eq!(model.catalogs[0].album_level, None);
    assert_eq!(model.catalogs[0].albums_level.listing, next_listing);
}

#[test]
fn enter_on_a_playlist_track_plays_it_with_the_playlist_as_the_play_order() {
    let mut model = playlists_model();
    model.catalogs[0].album_level = Some(BrowseLevel {
        listing: Listing::Playlist(PlaylistId::new("pl-0")),
        catalog_rows: vec![track_row("t-0"), track_row("t-1"), track_row("t-2")],
        cursor: Cursor::at(3, 1),
        paging: Paging::Complete,
        server_query: None,
    });

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    assert!(answer.is_ok(), "{answer:?}");
    let CatalogRow::Track(selected) = track_row("t-1") else {
        panic!("track_row gives a track");
    };
    assert_eq!(model.playlist.tracks.len(), 3);
    assert_eq!(
        model.playlist.current().map(|track| track.source()),
        Some(selected.source())
    );
    assert_eq!(model.playlist_source, PlaylistSource::Server(home()));
}
