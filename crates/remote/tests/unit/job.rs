use std::{
    io,
    sync::{Arc, Once},
    time::Duration,
};

use kernel::domain::{
    favorites::{Favorite, Favorites},
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
        Page,
        PlaylistId,
        RemoteError,
        Secret,
        ServerAlbum,
        ServerName,
        ServerPlaylist,
        ServerTrackId,
        Session,
        UserName,
    },
    track::{
        AudioFormat,
        CatalogRow,
        Decibels,
        Hertz,
        Kbps,
        Tagging,
        Tags,
        Track,
        TrackSource,
    },
};
use remote::{
    http::agent,
    job::{KEYCHAIN_SERVICE, RemoteJob},
    message::RemoteMessage,
};

use crate::unit::subsonic::{FAILED_BODY, OK_BODY, answer, query_value, serve};

static MOCK_STORE: Once = Once::new();

fn mock_store() {
    MOCK_STORE.call_once(|| {
        if let Ok(store) = keyring_core::mock::Store::new() {
            keyring_core::set_default_store(store);
        }
    });
}

fn connection(link: &str, user: &str, credential: Credential) -> Option<Connection> {
    Some(Connection {
        account: Account {
            server_name: ServerName::new("home"),
            endpoint: Endpoint::parse(link).ok()?,
            user_name: UserName::new(user).ok()?,
        },
        credential,
    })
}

#[test]
fn a_forget_deletes_the_stored_password_and_a_missing_one_is_fine() {
    mock_store();
    let Some(account) =
        connection("https://forget.example", "dora", Credential::Stored)
            .map(|connection| connection.account)
    else {
        panic!("a valid account");
    };
    let saved = keyring_core::Entry::new(KEYCHAIN_SERVICE, &account.keychain_account())
        .and_then(|entry| entry.set_password("hunter2"));
    assert!(saved.is_ok());

    let first = RemoteJob::Forget(account.clone()).run(&agent());
    let second = RemoteJob::Forget(account.clone()).run(&agent());

    assert!(
        matches!(first, RemoteMessage::Forgotten(Ok(()))),
        "was {first:?}"
    );
    assert!(
        matches!(second, RemoteMessage::Forgotten(Ok(()))),
        "was {second:?}"
    );
    assert!(matches!(
        keyring_core::Entry::new(KEYCHAIN_SERVICE, &account.keychain_account())
            .and_then(|entry| entry.get_password()),
        Err(keyring_core::Error::NoEntry)
    ));
}

fn typed() -> Option<Credential> {
    Some(Credential::Typed(Secret::new("hunter2").ok()?))
}

#[test]
fn a_typed_password_is_saved_after_an_ok_ping_and_a_stored_connect_signs_with_it() {
    mock_store();
    let (link, handle) = serve(vec![
        answer("200 OK", "", OK_BODY),
        answer("200 OK", "", OK_BODY),
    ]);
    let (Some(first), Some(second)) = (
        typed().and_then(|credential| connection(&link, "bob", credential)),
        connection(&link, "bob", Credential::Stored),
    ) else {
        panic!("valid connections");
    };
    let RemoteMessage::Connected {
        result: typed_result,
        stored: typed_stored,
        ..
    } = RemoteJob::Connect(first).run(&agent())
    else {
        panic!("a connect job answers Connected");
    };
    assert!(typed_result.is_ok());
    assert_eq!(typed_stored, Ok(()));
    let RemoteMessage::Connected {
        server_name,
        result,
        stored,
    } = RemoteJob::Connect(second).run(&agent())
    else {
        panic!("a connect job answers Connected");
    };
    assert_eq!(server_name, ServerName::new("home"));
    assert_eq!(stored, Ok(()));
    let query = result.map(|session| session.query.to_string());
    let value = |key| {
        query
            .as_deref()
            .ok()
            .and_then(|query| query_value(query, key))
    };
    let token =
        value("s").map(|salt| format!("{:x}", md5::compute(format!("hunter2{salt}"))));
    assert_eq!(value("u"), Some("bob"));
    assert_eq!(value("t"), token.as_deref());
    assert_eq!(handle.join().map(|requests| requests.len()).ok(), Some(2));
}

#[test]
fn a_refused_typed_password_is_not_saved() {
    mock_store();
    let (link, handle) = serve(vec![answer("200 OK", "", FAILED_BODY)]);
    let (Some(first), Some(second)) = (
        typed().and_then(|credential| connection(&link, "carol", credential)),
        connection(&link, "carol", Credential::Stored),
    ) else {
        panic!("valid connections");
    };
    let RemoteMessage::Connected { result, stored, .. } =
        RemoteJob::Connect(first).run(&agent())
    else {
        panic!("a connect job answers Connected");
    };
    assert_eq!(
        result.err(),
        Some(RemoteError::Api {
            server_name: ServerName::new("home"),
            api_code: ApiCode(40),
        })
    );
    assert_eq!(stored, Ok(()));
    let RemoteMessage::Connected {
        result: stored_result,
        ..
    } = RemoteJob::Connect(second).run(&agent())
    else {
        panic!("a connect job answers Connected");
    };
    assert_eq!(
        stored_result.err(),
        Some(RemoteError::NoPassword {
            server_name: ServerName::new("home"),
        })
    );
    assert_eq!(handle.join().map(|requests| requests.len()).ok(), Some(1));
}

#[test]
fn the_keychain_service_is_the_signing_identifier() {
    assert_eq!(KEYCHAIN_SERVICE, "dev.zefiro");
}

#[test]
fn a_stored_read_the_keychain_denies_without_asking_needs_a_password() {
    mock_store();
    let Some(connection) =
        connection("https://denied.example", "erin", Credential::Stored)
    else {
        panic!("a valid connection");
    };
    let entry = keyring_core::Entry::new(
        KEYCHAIN_SERVICE,
        &connection.account.keychain_account(),
    );
    let Some(cred) = entry
        .as_ref()
        .ok()
        .and_then(|entry| entry.as_any().downcast_ref::<keyring_core::mock::Cred>())
    else {
        panic!("a mock credential");
    };
    cred.set_error(keyring_core::Error::PlatformFailure(Box::new(
        io::Error::other("User interaction is not allowed."),
    )));

    let answer = RemoteJob::Connect(connection).run(&agent());

    let RemoteMessage::Connected { result, stored, .. } = answer else {
        panic!("a connect job answers Connected");
    };
    assert_eq!(
        result.err(),
        Some(RemoteError::NoPassword {
            server_name: ServerName::new("home"),
        })
    );
    assert_eq!(stored, Ok(()));
}

#[test]
fn a_saved_password_drops_the_item_of_the_old_service() {
    mock_store();
    let (link, handle) = serve(vec![answer("200 OK", "", OK_BODY)]);
    let Some(connection) =
        typed().and_then(|credential| connection(&link, "fay", credential))
    else {
        panic!("a valid connection");
    };
    let account = connection.account.clone();
    let old = keyring_core::Entry::new("zefiro", &account.keychain_account())
        .and_then(|entry| entry.set_password("old"));
    assert!(old.is_ok());

    let answer = RemoteJob::Connect(connection).run(&agent());

    assert!(
        matches!(answer, RemoteMessage::Connected { stored: Ok(()), .. }),
        "was {answer:?}"
    );
    assert_eq!(
        keyring_core::Entry::new(KEYCHAIN_SERVICE, &account.keychain_account())
            .and_then(|entry| entry.get_password())
            .ok()
            .as_deref(),
        Some("hunter2")
    );
    assert!(matches!(
        keyring_core::Entry::new("zefiro", &account.keychain_account())
            .and_then(|entry| entry.get_password()),
        Err(keyring_core::Error::NoEntry)
    ));
    assert_eq!(handle.join().map(|requests| requests.len()).ok(), Some(1));
}

const ALBUM_LIST_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.53.3 (13af8ed4)","openSubsonic":true,"albumList2":{"album":[{"id":"3a1f9c","name":"Kind of Blue","artist":"Miles Davis","artistId":"7b2e","coverArt":"al-3a1f9c_0","songCount":5,"duration":2760,"playCount":12,"created":"2024-03-01T10:00:00Z","year":1959,"genre":"Jazz","isDir":true,"isVideo":false,"mediaType":"album"},{"id":"9d0e"}]}}}"#;

const ALBUM_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.53.3 (13af8ed4)","openSubsonic":true,"album":{"id":"3a1f9c","name":"Kind of Blue","artist":"Miles Davis","songCount":2,"duration":900,"song":[{"id":"c41d","parent":"3a1f9c","isDir":false,"title":"So What","album":"Kind of Blue","artist":"Miles Davis","track":1,"year":1959,"genre":"Jazz","coverArt":"mf-c41d_0","size":21000000,"contentType":"audio/flac","suffix":"flac","duration":562,"bitRate":1016,"samplingRate":44100,"bitDepth":16,"channelCount":2,"discNumber":1,"starred":"2024-05-01T10:00:00Z","path":"Miles Davis/Kind of Blue/01 - So What.flac","replayGain":{"trackGain":-6.5,"albumGain":-7.1,"trackPeak":0.98}},{"id":"e7a2","title":"Freddie Freeloader"}]}}}"#;

fn list(link: &str, listing: Listing, page: Page) -> Option<RemoteJob> {
    Some(RemoteJob::List {
        server_name: ServerName::new("home"),
        session: Session::new(Endpoint::parse(link).ok()?, "u=bob&t=token&s=salt"),
        listing,
        page,
        revision: Revision::default().next(),
    })
}

fn listed(
    body: &str,
    listing: Listing,
    page: Page,
) -> (Option<RemoteMessage>, Vec<String>) {
    let (link, handle) = serve(vec![answer("200 OK", "", body)]);
    let message = list(&link, listing, page).map(|job| job.run(&agent()));
    (message, handle.join().unwrap_or_else(|_panic| Vec::new()))
}

#[test]
fn a_list_page_asks_for_two_hundred_albums_from_its_first_row_and_turns_them_into_rows()
{
    let (message, requests) = listed(
        ALBUM_LIST_BODY,
        Listing::Albums(AlbumOrder::Newest),
        Page(2),
    );
    let request = requests.first().map_or("", String::as_str);
    let query = request.split_once('?').map_or("", |(_path, query)| query);
    assert!(request.starts_with("GET /rest/getAlbumList2?"));
    assert_eq!(query_value(query, "type"), Some("newest"));
    assert_eq!(query_value(query, "size"), Some("200"));
    assert_eq!(query_value(query, "offset"), Some("400"));
    let Some(RemoteMessage::Listed {
        server_name,
        listing,
        page,
        result,
        revision,
    }) = message
    else {
        panic!("a list job answers Listed");
    };
    assert_eq!(server_name, ServerName::new("home"));
    assert_eq!(listing, Listing::Albums(AlbumOrder::Newest));
    assert_eq!(page, Page(2));
    assert_eq!(revision, Revision::default().next());
    assert_eq!(
        result,
        Ok((
            vec![
                CatalogRow::Album(ServerAlbum {
                    album_id: AlbumId::new("3a1f9c"),
                    title: Arc::from("Kind of Blue"),
                    artist: Arc::from("Miles Davis"),
                    year: Some(1959),
                    track_count: 5,
                    duration: Duration::from_secs(2760),
                }),
                CatalogRow::Album(ServerAlbum {
                    album_id: AlbumId::new("9d0e"),
                    title: Arc::from(""),
                    artist: Arc::from(""),
                    year: None,
                    track_count: 0,
                    duration: Duration::ZERO,
                }),
            ],
            Favorites::default()
        ))
    );
}

const SONGS_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{"song":[{"id":"c41d","title":"So What","artist":"Miles Davis","duration":562,"starred":"2024-05-01T10:00:00Z"},{"id":"e7a2","title":"Freddie Freeloader"}]}}}"#;

#[test]
fn a_songs_page_asks_search3_for_two_hundred_songs_from_its_first_row_and_turns_them_into_tracks()
 {
    let (message, requests) = listed(SONGS_BODY, Listing::Songs, Page(1));
    let request = requests.first().map_or("", String::as_str);
    let query = request.split_once('?').map_or("", |(_path, query)| query);
    assert!(request.starts_with("GET /rest/search3?"), "{request}");
    assert_eq!(query_value(query, "query"), Some(""));
    assert_eq!(query_value(query, "artistCount"), Some("0"));
    assert_eq!(query_value(query, "albumCount"), Some("0"));
    assert_eq!(query_value(query, "songCount"), Some("200"));
    assert_eq!(query_value(query, "songOffset"), Some("200"));
    let Some(RemoteMessage::Listed {
        listing,
        result: Ok((catalog_rows, favorites)),
        ..
    }) = message
    else {
        panic!("a songs page answers Listed with rows");
    };
    assert_eq!(listing, Listing::Songs);
    let sources: Vec<&TrackSource> = tracks(&catalog_rows)
        .into_iter()
        .map(Track::source)
        .collect();
    let first_source = TrackSource::Server {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new("c41d"),
    };
    let second_source = TrackSource::Server {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new("e7a2"),
    };
    assert_eq!(sources, vec![&first_source, &second_source]);
    assert_eq!(favorites.favorite(&first_source), Favorite::Yes);
    assert_eq!(favorites.favorite(&second_source), Favorite::No);
}

fn tracks(catalog_rows: &[CatalogRow]) -> Vec<&Track> {
    catalog_rows
        .iter()
        .filter_map(|catalog_row| match catalog_row {
            CatalogRow::Track(track) => Some(track.as_ref()),
            CatalogRow::Album(_server_album) => None,
            CatalogRow::Playlist(_server_playlist) => None,
        })
        .collect()
}

const PLAYLISTS_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","openSubsonic":true,"playlists":{"playlist":[{"id":"800","name":"Late Night","comment":"","owner":"bob","public":false,"songCount":12,"duration":3120,"created":"2024-03-01T10:00:00Z","changed":"2024-05-01T10:00:00Z","coverArt":"pl-800"},{"id":"801"}]}}}"#;

#[test]
fn playlists_asks_get_playlists_and_gives_each_its_name_track_count_and_duration() {
    let (message, requests) =
        listed(PLAYLISTS_BODY, Listing::Playlists, Page::default());
    let request = requests.first().map_or("", String::as_str);
    assert!(
        request.starts_with("GET /rest/getPlaylists?u=bob&"),
        "{request}"
    );
    let Some(RemoteMessage::Listed {
        listing, result, ..
    }) = message
    else {
        panic!("a playlists list answers Listed");
    };
    assert_eq!(listing, Listing::Playlists);
    assert_eq!(
        result,
        Ok((
            vec![
                CatalogRow::Playlist(ServerPlaylist {
                    playlist_id: PlaylistId::new("800"),
                    name: Arc::from("Late Night"),
                    track_count: 12,
                    duration: Duration::from_secs(3120),
                }),
                CatalogRow::Playlist(ServerPlaylist {
                    playlist_id: PlaylistId::new("801"),
                    name: Arc::from(""),
                    track_count: 0,
                    duration: Duration::ZERO,
                }),
            ],
            Favorites::default()
        ))
    );
}

const PLAYLIST_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","playlist":{"id":"800","name":"Late Night","songCount":2,"duration":1100,"entry":[{"id":"e7a2","title":"Freddie Freeloader"},{"id":"c41d","title":"So What","artist":"Miles Davis","duration":562,"starred":"2024-05-01T10:00:00Z"}]}}}"#;

#[test]
fn a_playlist_asks_get_playlist_by_its_id_and_gives_its_entries_as_tracks_in_order() {
    let (message, requests) = listed(
        PLAYLIST_BODY,
        Listing::Playlist(PlaylistId::new("800")),
        Page::default(),
    );
    let request = requests.first().map_or("", String::as_str);
    let query = request.split_once('?').map_or("", |(_path, query)| query);
    assert!(request.starts_with("GET /rest/getPlaylist?"), "{request}");
    assert_eq!(query_value(query, "id"), Some("800"));
    let Some(RemoteMessage::Listed {
        listing,
        result: Ok((catalog_rows, favorites)),
        ..
    }) = message
    else {
        panic!("a playlist answers Listed with rows");
    };
    assert_eq!(listing, Listing::Playlist(PlaylistId::new("800")));
    let sources: Vec<&TrackSource> = tracks(&catalog_rows)
        .into_iter()
        .map(Track::source)
        .collect();
    let first_source = TrackSource::Server {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new("e7a2"),
    };
    let second_source = TrackSource::Server {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new("c41d"),
    };
    assert_eq!(sources, vec![&first_source, &second_source]);
    assert_eq!(favorites.favorite(&first_source), Favorite::No);
    assert_eq!(favorites.favorite(&second_source), Favorite::Yes);
}

#[test]
fn an_album_becomes_tagged_server_tracks_and_a_song_without_replay_gain_has_no_decibels()
 {
    let (message, requests) = listed(
        ALBUM_BODY,
        Listing::Album(AlbumId::new("3a1f9c")),
        Page::default(),
    );
    let request = requests.first().map_or("", String::as_str);
    let query = request.split_once('?').map_or("", |(_path, query)| query);
    assert!(request.starts_with("GET /rest/getAlbum?"));
    assert_eq!(query_value(query, "id"), Some("3a1f9c"));
    let Some(RemoteMessage::Listed {
        result: Ok((catalog_rows, _favorites)),
        ..
    }) = message
    else {
        panic!("an album answers Listed rows");
    };
    let tracks = tracks(&catalog_rows);
    let [so_what, freeloader] = tracks.as_slice() else {
        panic!("two tracks, got {catalog_rows:?}");
    };
    assert_eq!(
        so_what.source(),
        &TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("c41d"),
        }
    );
    assert_eq!(
        so_what.tags(),
        &Tags {
            title: Some("So What".to_owned()),
            artist: Some("Miles Davis".to_owned()),
            album: Some("Kind of Blue".to_owned()),
            date: Some("1959".to_owned()),
            genre: Some("Jazz".to_owned()),
            track_number: Some(1),
            disc: Some(1),
            ..Tags::default()
        }
    );
    assert_eq!(
        so_what.audio_format(),
        &AudioFormat {
            format: Some("flac".to_owned()),
            bitrate: Some(Kbps(1016)),
            sample_rate: Some(Hertz(44100)),
            bits_per_sample: Some(16),
            channels: Some(2),
            decibels: Some(Decibels(-6.5)),
        }
    );
    assert_eq!(so_what.tagging(), Tagging::Tagged(Duration::from_secs(562)));
    assert_eq!(
        (so_what.display(), freeloader.display()),
        ("Miles Davis — So What", "Freddie Freeloader")
    );
    assert_eq!(freeloader.audio_format(), &AudioFormat::default());
    assert_eq!(freeloader.tagging(), Tagging::Tagged(Duration::ZERO));
}

#[test]
fn a_song_with_starred_is_a_favorite_and_a_song_without_it_is_not() {
    let (message, _requests) = listed(
        ALBUM_BODY,
        Listing::Album(AlbumId::new("3a1f9c")),
        Page::default(),
    );
    let Some(RemoteMessage::Listed {
        result: Ok((_catalog_rows, favorites)),
        ..
    }) = message
    else {
        panic!("an album answers Listed");
    };
    let song = |id| TrackSource::Server {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new(id),
    };
    assert_eq!(favorites.favorite(&song("c41d")), Favorite::Yes);
    assert_eq!(favorites.favorite(&song("e7a2")), Favorite::No);
}

#[test]
fn an_entry_without_an_id_is_a_parse_error() {
    let (message, _requests) = listed(
        r#"{"subsonic-response":{"status":"ok","albumList2":{"album":[{"name":"No id"}]}}}"#,
        Listing::Albums(AlbumOrder::Random),
        Page::default(),
    );
    assert!(matches!(
        message,
        Some(RemoteMessage::Listed {
            result: Err(RemoteError::Parse { .. }),
            ..
        })
    ));
}

const SEARCH_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.53.3 (13af8ed4)","openSubsonic":true,"searchResult3":{"song":[{"id":"c41d","parent":"3a1f9c","title":"So What","album":"Kind of Blue","artist":"Miles Davis","duration":562,"starred":"2024-05-01T10:00:00Z"}],"album":[{"id":"3a1f9c","name":"Kind of Blue","artist":"Miles Davis","songCount":5,"duration":2760,"year":1959}]}}}"#;

#[test]
fn a_search_gets_its_link_and_gives_albums_before_tracks() {
    let (link, handle) = serve(vec![answer("200 OK", "", SEARCH_BODY)]);
    let message = Some(
        RemoteJob::Search {
            server_name: ServerName::new("home"),
            link: format!("{link}/rest/search3?query=kind%20of&u=bob"),
            revision: Revision::default().next(),
        }
        .run(&agent()),
    );
    let requests = handle.join().unwrap_or_else(|_panic| Vec::new());
    let request = requests.first().map_or("", String::as_str);
    assert!(request.starts_with("GET /rest/search3?query=kind%20of&u=bob "));
    let Some(RemoteMessage::Found {
        server_name,
        result: Ok((catalog_rows, favorites)),
        revision,
    }) = message
    else {
        panic!("a search job answers Found rows");
    };
    assert_eq!(server_name, ServerName::new("home"));
    assert_eq!(revision, Revision::default().next());
    assert_eq!(
        catalog_rows.first(),
        Some(&CatalogRow::Album(ServerAlbum {
            album_id: AlbumId::new("3a1f9c"),
            title: Arc::from("Kind of Blue"),
            artist: Arc::from("Miles Davis"),
            year: Some(1959),
            track_count: 5,
            duration: Duration::from_secs(2760),
        }))
    );
    let tracks = tracks(&catalog_rows);
    let [so_what] = tracks.as_slice() else {
        panic!("one track after the album, got {catalog_rows:?}");
    };
    assert_eq!(catalog_rows.len(), 2);
    assert_eq!(
        so_what.source(),
        &TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("c41d"),
        }
    );
    assert_eq!(favorites, [so_what.source().clone()].into_iter().collect());
}
