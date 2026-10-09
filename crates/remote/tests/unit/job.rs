use std::{
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
        RemoteError,
        Secret,
        ServerAlbum,
        ServerName,
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
use remote::{http::agent, job::RemoteJob, message::RemoteMessage};

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
    let saved = keyring_core::Entry::new("zefiro", &account.keychain_account())
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
        keyring_core::Entry::new("zefiro", &account.keychain_account())
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

fn tracks(catalog_rows: &[CatalogRow]) -> Vec<&Track> {
    catalog_rows
        .iter()
        .filter_map(|catalog_row| match catalog_row {
            CatalogRow::Track(track) => Some(track.as_ref()),
            CatalogRow::Album(_server_album) => None,
        })
        .collect()
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
fn a_search_asks_search3_for_albums_and_songs_and_gives_albums_before_tracks() {
    let (link, handle) = serve(vec![answer("200 OK", "", SEARCH_BODY)]);
    let message = Endpoint::parse(&link).ok().map(|endpoint| {
        RemoteJob::Search {
            server_name: ServerName::new("home"),
            session: Session::new(endpoint, "u=bob&t=token&s=salt"),
            input: "kind of".to_owned(),
            revision: Revision::default().next(),
        }
        .run(&agent())
    });
    let requests = handle.join().unwrap_or_else(|_panic| Vec::new());
    let request = requests.first().map_or("", String::as_str);
    let query = request.split_once('?').map_or("", |(_path, query)| query);
    assert!(request.starts_with("GET /rest/search3?"));
    assert_eq!(query_value(query, "query"), Some("kind%20of"));
    assert_eq!(query_value(query, "artistCount"), Some("0"));
    assert_eq!(query_value(query, "albumCount"), Some("20"));
    assert_eq!(query_value(query, "songCount"), Some("50"));
    assert_eq!(query_value(query, "u"), Some("bob"));
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
