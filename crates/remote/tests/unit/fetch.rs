use std::{
    env,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process,
    sync::Arc,
    thread,
    time::{Duration, SystemTime},
};

use kernel::domain::{
    io_error::IoError,
    revision::Revision,
    server::{
        ApiCode,
        CacheKey,
        Endpoint,
        Fetched,
        MediaFetch,
        RemoteError,
        ServerName,
        ServerTrackId,
        Session,
    },
};
use remote::{http::agent, job::RemoteJob, message::RemoteMessage};
use rstest::rstest;

use crate::unit::subsonic::{answer, serve};

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

#[rstest]
#[case::whole_without_range(Served::Whole, "home/tr-1.flac", 9 * MIB)]
#[case::cut_connection(Served::Cut, "home/tr-1.flac.part", MIB)]
fn one_answer_leaves_downloaded_at_the_last_written_byte(
    #[case] served: Served,
    #[case] written: &str,
    #[case] downloaded: u64,
) {
    let media = media();
    let (link, handle) = serve_media(&media, served, 1);
    let media_dir = media_dir(&format!("{served:?}"));
    let media_path = media_dir.join(written);

    let answer = fetched(media_fetch(&link, 0), &media_dir, Vec::new());

    assert_eq!(
        answer,
        Some(Ok(Fetched {
            media_path: media_path.clone(),
            downloaded,
            byte_len: 9 * MIB,
        }))
    );
    assert_eq!(
        fs::read(&media_path).ok().as_deref(),
        usize::try_from(downloaded)
            .ok()
            .and_then(|len| media.get(..len))
    );
    assert!(handle.join().is_ok());
    assert!(fs::remove_dir_all(&media_dir).is_ok());
}

#[rstest]
#[case::empty_whole_file(
    "empty-whole",
    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    |media_dir: &Path| Ok(Fetched {
        media_path: media_dir.join("home/tr-1.flac"),
        downloaded: 0,
        byte_len: 0,
    })
)]
#[case::empty_range(
    "empty-range",
    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-99/100\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    |_media_dir: &Path| Err(RemoteError::Unreachable {
        server_name: ServerName::new("home"),
        source: IoError::Malformed,
    })
)]
fn an_answer_without_bytes_is_complete_only_for_an_empty_file(
    #[case] name: &str,
    #[case] reply: &str,
    #[case] expected: fn(&Path) -> Result<Fetched, RemoteError>,
) {
    let (link, handle) = serve(vec![reply.to_owned()]);
    let media_dir = media_dir(name);

    let answer = fetched(media_fetch(&link, 0), &media_dir, Vec::new());

    assert_eq!(answer, Some(expected(&media_dir)));
    assert!(handle.join().is_ok_and(|requests| requests.len() == 1));
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

struct EvictionRow {
    oldest: fn(&Path) -> std::io::Result<()>,
    len: u64,
    refusal: fn(&RemoteError) -> bool,
    left: Vec<bool>,
}

#[rstest]
#[case::files_at_the_cache_bytes(EvictionRow {
    oldest: |path| {
        let file = fs::File::create(path)?;
        file.set_len(512 * MIB)?;
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000))
    },
    len: 512 * MIB,
    refusal: |answer| matches!(answer, RemoteError::Unreachable { .. }),
    left: vec![false, true, true, true],
})]
#[case::dangling_link(EvictionRow {
    oldest: |path| std::os::unix::fs::symlink(path.with_extension("gone"), path),
    len: 700 * MIB,
    refusal: |answer| matches!(answer, RemoteError::Unreachable { .. }),
    left: vec![true, false, true, true],
})]
#[case::directory_in_the_way(EvictionRow {
    oldest: |path| {
        fs::create_dir(path)?;
        fs::File::open(path)?.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000))
    },
    len: 700 * MIB,
    refusal: |answer| matches!(answer, RemoteError::Cache { .. }),
    left: vec![true, true, true, true],
})]
fn eviction_frees_below_the_cache_bytes_past_a_dangling_link_and_stops_at_a_failed_removal(
    #[case] row: EvictionRow,
) {
    let EvictionRow {
        oldest,
        len,
        refusal,
        left,
    } = row;
    let media_dir = media_dir(&format!(
        "evict-{len}-{}",
        left.iter().filter(|kept| **kept).count()
    ));
    assert!(fs::create_dir_all(media_dir.join("home")).is_ok());
    let paths: Vec<PathBuf> = ["oldest", "old", "new", "newest"]
        .into_iter()
        .map(|id| media_dir.join(cache_key(id).as_str()))
        .collect();
    let Some((oldest_path, newer_paths)) = paths.split_first() else {
        return;
    };
    let made_oldest = oldest(oldest_path);
    assert!(made_oldest.is_ok(), "{made_oldest:?}");
    for (age, path) in (1..).zip(newer_paths) {
        let made = fs::File::create(path).and_then(|file| {
            file.set_len(len)?;
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + age))
        });
        assert!(made.is_ok(), "{made:?}");
    }

    let answer = fetched(media_fetch(&closed_link(), 0), &media_dir, Vec::new());

    assert!(
        answer
            .as_ref()
            .is_some_and(|result| result.as_ref().is_err_and(refusal)),
        "{answer:?}"
    );
    let kept: Vec<bool> = paths
        .iter()
        .map(|path| fs::symlink_metadata(path).is_ok())
        .collect();
    assert_eq!(kept, left);
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
