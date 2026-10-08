use std::{
    env,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process,
    sync::{Arc, Once},
    thread,
    time::{Duration, SystemTime},
};

use kernel::domain::{
    revision::Revision,
    server::{
        Account,
        AlbumId,
        AlbumOrder,
        ApiCode,
        CacheKey,
        Connection,
        Credential,
        Endpoint,
        Fetched,
        Listing,
        MediaFetch,
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

const ALBUM_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.53.3 (13af8ed4)","openSubsonic":true,"album":{"id":"3a1f9c","name":"Kind of Blue","artist":"Miles Davis","songCount":2,"duration":900,"song":[{"id":"c41d","parent":"3a1f9c","isDir":false,"title":"So What","album":"Kind of Blue","artist":"Miles Davis","track":1,"year":1959,"genre":"Jazz","coverArt":"mf-c41d_0","size":21000000,"contentType":"audio/flac","suffix":"flac","duration":562,"bitRate":1016,"samplingRate":44100,"bitDepth":16,"channelCount":2,"discNumber":1,"path":"Miles Davis/Kind of Blue/01 - So What.flac","replayGain":{"trackGain":-6.5,"albumGain":-7.1,"trackPeak":0.98}},{"id":"e7a2","title":"Freddie Freeloader"}]}}}"#;

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
        Ok(vec![
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
        ])
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
        result: Ok(catalog_rows),
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

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
enum Served {
    Ranged,
    Whole,
    Cut,
}

fn media() -> Vec<u8> {
    (0..=250_u8).cycle().take(9 * 1024 * 1024).collect()
}

fn requested_range(stream: &TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    let mut range = String::new();
    reader.read_line(&mut line).ok()?;
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? <= 2 {
            return Some(range);
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:") {
            value.trim().clone_into(&mut range);
        }
    }
}

fn reply(media: &[u8], served: Served, range: &str) -> Vec<u8> {
    let total = media.len();
    let (first, last) = range
        .strip_prefix("bytes=")
        .and_then(|span| span.split_once('-'))
        .and_then(|(first, last)| {
            Some((first.parse::<usize>().ok()?, last.parse::<usize>().ok()?))
        })
        .unwrap_or((0, total));
    let last = last.min(total.saturating_sub(1));
    let part = media.get(first..=last).unwrap_or(&[]);
    let partial = |sent: &[u8]| {
        let head = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {first}-{last}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            part.len()
        );
        [head.as_bytes(), sent].concat()
    };
    match served {
        Served::Ranged => partial(part),
        Served::Cut => partial(part.get(..1024 * 1024).unwrap_or(part)),
        Served::Whole => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
            );
            [head.as_bytes(), media].concat()
        }
    }
}

fn serve_media(
    media: &[u8],
    served: Served,
    requests: usize,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        return (String::new(), thread::spawn(Vec::new));
    };
    let port = listener.local_addr().map_or(0, |addr| addr.port());
    let media = media.to_vec();
    let handle = thread::spawn(move || {
        (0..requests)
            .filter_map(|_| {
                let (mut stream, _addr) = listener.accept().ok()?;
                let range = requested_range(&stream)?;
                stream.write_all(&reply(&media, served, &range)).ok()?;
                Some(range)
            })
            .collect()
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn closed_link() -> String {
    let (link, handle) = serve(Vec::new());
    assert!(handle.join().is_ok());
    link
}

fn media_dir(name: &str) -> PathBuf {
    let media_dir =
        env::temp_dir().join(format!("sifr-media-{}-{name}", process::id()));
    assert!(!media_dir.exists() || fs::remove_dir_all(&media_dir).is_ok());
    media_dir
}

fn cache_key(id: &str) -> CacheKey {
    CacheKey::new(&ServerName::new("home"), &ServerTrackId::new(id), "flac")
}

fn media_fetch(link: &str, first_byte: u64) -> Option<MediaFetch> {
    Some(MediaFetch {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new("tr-1"),
        cache_key: cache_key("tr-1"),
        session: Session::new(Endpoint::parse(link).ok()?, "u=ann&t=token&s=salt"),
        first_byte,
        revision: Revision::default(),
    })
}

fn fetched(
    media_fetch: Option<MediaFetch>,
    media_dir: &Path,
    kept_cache_keys: Vec<CacheKey>,
) -> Option<Result<Fetched, RemoteError>> {
    let RemoteMessage::Fetched {
        revision: _revision,
        result,
    } = (RemoteJob::Fetch {
        media_fetch: media_fetch?,
        media_dir: Arc::from(media_dir),
        kept_cache_keys,
    })
    .run(&agent())
    else {
        return None;
    };
    Some(result)
}

#[test]
fn a_ranged_server_fills_the_file_in_three_chunks() {
    let media = media();
    let (link, handle) = serve_media(&media, Served::Ranged, 3);
    let media_dir = media_dir("ranged");
    let part_path = media_dir.join("home/tr-1.flac.part");
    let media_path = media_dir.join("home/tr-1.flac");

    let answers: Option<Vec<_>> = [0, 4 * MIB, 8 * MIB]
        .into_iter()
        .map(|first_byte| {
            fetched(media_fetch(&link, first_byte), &media_dir, Vec::new())
        })
        .collect();

    assert_eq!(
        answers,
        Some(vec![
            Ok(Fetched {
                media_path: part_path.clone(),
                downloaded: 4 * MIB,
                byte_len: 9 * MIB,
            }),
            Ok(Fetched {
                media_path: part_path.clone(),
                downloaded: 8 * MIB,
                byte_len: 9 * MIB,
            }),
            Ok(Fetched {
                media_path: media_path.clone(),
                downloaded: 9 * MIB,
                byte_len: 9 * MIB,
            }),
        ])
    );
    assert_eq!(fs::read(&media_path).ok(), Some(media));
    assert!(!part_path.exists());
    assert_eq!(
        handle.join().ok(),
        Some(vec![
            "bytes=0-4194303".to_owned(),
            "bytes=4194304-8388607".to_owned(),
            "bytes=8388608-12582911".to_owned(),
        ])
    );
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

#[test]
fn a_server_without_range_gives_the_whole_file_at_once() {
    let media = media();
    let (link, handle) = serve_media(&media, Served::Whole, 1);
    let media_dir = media_dir("whole");
    let media_path = media_dir.join("home/tr-1.flac");

    let answer = fetched(media_fetch(&link, 0), &media_dir, Vec::new());

    assert_eq!(
        answer,
        Some(Ok(Fetched {
            media_path: media_path.clone(),
            downloaded: 9 * MIB,
            byte_len: 9 * MIB,
        }))
    );
    assert_eq!(fs::read(&media_path).ok(), Some(media));
    assert!(handle.join().is_ok());
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

#[test]
fn a_cut_connection_leaves_downloaded_at_the_last_written_byte() {
    let media = media();
    let (link, handle) = serve_media(&media, Served::Cut, 1);
    let media_dir = media_dir("cut");
    let part_path = media_dir.join("home/tr-1.flac.part");

    let answer = fetched(media_fetch(&link, 0), &media_dir, Vec::new());

    assert_eq!(
        answer,
        Some(Ok(Fetched {
            media_path: part_path.clone(),
            downloaded: MIB,
            byte_len: 9 * MIB,
        }))
    );
    assert_eq!(
        fs::metadata(&part_path).map(|metadata| metadata.len()).ok(),
        Some(MIB)
    );
    assert!(handle.join().is_ok());
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

#[test]
fn eviction_drops_the_oldest_files_and_keeps_the_latest_fetch_and_prefetch() {
    let media_dir = media_dir("evict");
    assert!(fs::create_dir_all(media_dir.join("home")).is_ok());
    let ids = [
        "kept_cache_keys-a",
        "kept_cache_keys-b",
        "old",
        "older-than-new",
        "new",
    ];
    for (age, id) in (0..).zip(ids) {
        let made =
            fs::File::create(media_dir.join(cache_key(id).as_str())).and_then(|file| {
                file.set_len(600 * MIB)?;
                file.set_modified(
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + age),
                )
            });
        assert!(made.is_ok(), "{made:?}");
    }

    let answer = fetched(
        media_fetch(&closed_link(), 0),
        &media_dir,
        vec![
            cache_key("kept_cache_keys-a"),
            cache_key("kept_cache_keys-b"),
        ],
    );

    assert!(
        matches!(answer, Some(Err(RemoteError::Unreachable { .. }))),
        "{answer:?}"
    );
    let left: Vec<bool> = ids
        .iter()
        .map(|id| media_dir.join(cache_key(id).as_str()).exists())
        .collect();
    assert_eq!(left, vec![true, true, false, false, true]);
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

#[test]
fn a_cached_file_answers_complete_with_no_request_and_is_touched() {
    let media_dir = media_dir("cached");
    let media_path = media_dir.join("home/tr-1.flac");
    let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    assert!(fs::create_dir_all(media_dir.join("home")).is_ok());
    let made = fs::File::create(&media_path).and_then(|mut file| {
        file.write_all(b"flac!")?;
        file.set_modified(long_ago)
    });
    assert!(made.is_ok(), "{made:?}");

    let answer = fetched(media_fetch(&closed_link(), 0), &media_dir, Vec::new());

    assert_eq!(
        answer,
        Some(Ok(Fetched {
            media_path: media_path.clone(),
            downloaded: 5,
            byte_len: 5,
        }))
    );
    assert!(
        fs::metadata(&media_path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified > long_ago)
    );
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

#[test]
fn eviction_drops_an_old_stray_part_and_keeps_the_parts_of_kept_downloads() {
    let media_dir = media_dir("evict-part");
    assert!(fs::create_dir_all(media_dir.join("home")).is_ok());
    let paths: Vec<PathBuf> = [
        ("kept_cache_keys-a", ".part"),
        ("stray", ".part"),
        ("old", ""),
        ("new", ""),
    ]
    .into_iter()
    .map(|(id, suffix)| media_dir.join(format!("{}{suffix}", cache_key(id).as_str())))
    .collect();
    for (age, path) in (0..).zip(&paths) {
        let made = fs::File::create(path).and_then(|file| {
            file.set_len(600 * MIB)?;
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + age))
        });
        assert!(made.is_ok(), "{made:?}");
    }

    let answer = fetched(
        media_fetch(&closed_link(), 0),
        &media_dir,
        vec![cache_key("kept_cache_keys-a")],
    );

    assert!(
        matches!(answer, Some(Err(RemoteError::Unreachable { .. }))),
        "{answer:?}"
    );
    let left: Vec<bool> = paths.iter().map(|path| path.exists()).collect();
    assert_eq!(left, vec![true, false, true, true]);
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

const NOT_FOUND_BODY: &str = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"Song not found"}}}"#;

#[test]
fn an_error_answer_with_status_200_is_an_api_error_and_leaves_no_file() {
    let (link, handle) = serve(vec![answer("200 OK", "", NOT_FOUND_BODY)]);
    let media_dir = media_dir("not-found");

    let result = fetched(media_fetch(&link, 0), &media_dir, Vec::new());

    assert_eq!(
        result,
        Some(Err(RemoteError::Api {
            server_name: ServerName::new("home"),
            api_code: ApiCode(70),
        }))
    );
    assert!(!media_dir.join("home/tr-1.flac").exists());
    assert!(!media_dir.join("home/tr-1.flac.part").exists());
    assert!(handle.join().is_ok());
    assert!(!media_dir.exists() || fs::remove_dir_all(&media_dir).is_ok());
}

const SEARCH_BODY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.53.3 (13af8ed4)","openSubsonic":true,"searchResult3":{"song":[{"id":"c41d","parent":"3a1f9c","title":"So What","album":"Kind of Blue","artist":"Miles Davis","duration":562}],"album":[{"id":"3a1f9c","name":"Kind of Blue","artist":"Miles Davis","songCount":5,"duration":2760,"year":1959}]}}}"#;

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
        result: Ok(catalog_rows),
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
}
