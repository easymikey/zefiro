use std::{
    fs,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};

use kernel::domain::{
    config::Diagnostic,
    io_error::IoError,
    server::{
        Account,
        AlbumId,
        AlbumOrder,
        ApiCode,
        Connection,
        Endpoint,
        Fetched,
        HttpStatus,
        Listing,
        MediaFetch,
        PAGE_ROWS,
        Page,
        RemoteError,
        Secret,
        ServerAlbum,
        ServerName,
        ServerTrackId,
        Session,
    },
    track::{AudioFormat, CatalogRow, Decibels, Hertz, Kbps, Tags, Track, TrackSource},
};
use serde::Deserialize;
use serde_json::Value;
use ureq::{Agent, Body, RequestBuilder, http::Response, typestate::WithoutBody};

use crate::http::{API_BYTES, FETCH_CHUNK, FETCH_TIMEOUT};

const READ_BYTES: usize = 64 * 1024;

const API_VERSION: &str = "1.16.1";

const MAX_REDIRECTS: usize = 3;

const REDIRECT_STATUSES: [u16; 4] = [301, 302, 307, 308];

pub fn ping(
    agent: &Agent,
    connection: &Connection,
    secret: &Secret,
) -> Result<Session, RemoteError> {
    let account = &connection.account;
    let session = signed(account, secret);
    let link = format!("{}/rest/ping?{}", session.endpoint, session.query);
    fetched(&account.server_name, link, |current| agent.get(current))
        .and_then(|response| answer(&account.server_name, response))
        .map(|_answer| session)
}

pub(crate) fn answer(
    server_name: &ServerName,
    mut response: Response<Body>,
) -> Result<Value, RemoteError> {
    let body = response
        .body_mut()
        .with_config()
        .limit(API_BYTES)
        .read_to_vec()
        .map_err(|error| {
            if matches!(error, ureq::Error::BodyExceedsLimit(_limit)) {
                read_error(server_name, &error)
            } else {
                RemoteError::Unreachable {
                    server_name: server_name.clone(),
                    source: IoError::from(error.into_io().kind()),
                }
            }
        })?;
    let answer: Value = serde_json::from_slice(&body)
        .map_err(|error| read_error(server_name, &error))?;
    let status_value = answer
        .pointer("/subsonic-response/status")
        .unwrap_or(&Value::Null);
    let api_status = <&str>::deserialize(status_value)
        .map_err(|error| read_error(server_name, &error))?;
    if api_status == "ok" {
        return Ok(answer);
    }
    let code_value = answer
        .pointer("/subsonic-response/error/code")
        .unwrap_or(&Value::Null);
    let code = u16::deserialize(code_value)
        .map_err(|error| read_error(server_name, &error))?;
    Err(RemoteError::Api {
        server_name: server_name.clone(),
        api_code: ApiCode(code),
    })
}

pub(crate) fn query(listing: &Listing, page: Page) -> String {
    match listing {
        Listing::Albums(album_order) => {
            let album_type = match album_order {
                AlbumOrder::Newest => "newest",
                AlbumOrder::Recent => "recent",
                AlbumOrder::Frequent => "frequent",
                AlbumOrder::Starred => "starred",
                AlbumOrder::Alphabetical => "alphabeticalByName",
                AlbumOrder::Random => "random",
            };
            let first_row = page.0.saturating_mul(PAGE_ROWS);
            format!(
                "getAlbumList2?type={album_type}&size={PAGE_ROWS}&offset={first_row}"
            )
        }
        Listing::Album(album_id) => {
            format!("getAlbum?id={}", encoded(album_id.as_str()))
        }
    }
}

pub(crate) fn catalog_rows(
    server_name: &ServerName,
    listing: &Listing,
    answer: &Value,
) -> Result<Vec<CatalogRow>, RemoteError> {
    match listing {
        Listing::Albums(_album_order) => {
            albums(server_name, answer, "/subsonic-response/albumList2/album").collect()
        }
        Listing::Album(_album_id) => {
            tracks(server_name, answer, "/subsonic-response/album/song").collect()
        }
    }
}

pub(crate) fn search_query(input: &str, session: &Session) -> String {
    format!(
        "{}/rest/search3?query={}&artistCount=0&albumCount=20&songCount=50&{}",
        session.endpoint,
        encoded(input),
        session.query
    )
}

pub(crate) fn search_rows(
    server_name: &ServerName,
    answer: &Value,
) -> Result<Vec<CatalogRow>, RemoteError> {
    albums(
        server_name,
        answer,
        "/subsonic-response/searchResult3/album",
    )
    .chain(tracks(
        server_name,
        answer,
        "/subsonic-response/searchResult3/song",
    ))
    .collect()
}

fn albums<'a>(
    server_name: &'a ServerName,
    answer: &'a Value,
    pointer: &'a str,
) -> impl Iterator<Item = Result<CatalogRow, RemoteError>> + 'a {
    entries(answer, pointer)
        .map(|album| server_album(server_name, album).map(CatalogRow::Album))
}

fn tracks<'a>(
    server_name: &'a ServerName,
    answer: &'a Value,
    pointer: &'a str,
) -> impl Iterator<Item = Result<CatalogRow, RemoteError>> + 'a {
    entries(answer, pointer).map(|record| {
        track(server_name, record).map(|track| CatalogRow::Track(Arc::new(track)))
    })
}

fn entries<'a>(answer: &'a Value, pointer: &str) -> impl Iterator<Item = &'a Value> {
    answer
        .pointer(pointer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn server_album(
    server_name: &ServerName,
    album: &Value,
) -> Result<ServerAlbum, RemoteError> {
    Ok(ServerAlbum {
        album_id: AlbumId::new(id(server_name, album)?),
        title: Arc::from(field(album, "name").unwrap_or("")),
        artist: Arc::from(field(album, "artist").unwrap_or("")),
        year: field(album, "year"),
        track_count: field(album, "songCount").unwrap_or(0),
        duration: Duration::from_secs(field(album, "duration").unwrap_or(0)),
    })
}

fn track(server_name: &ServerName, record: &Value) -> Result<Track, RemoteError> {
    let source = TrackSource::Server {
        server_name: server_name.clone(),
        server_track_id: ServerTrackId::new(id(server_name, record)?),
    };
    let tags = Tags {
        title: field(record, "title"),
        artist: field(record, "artist"),
        album: field(record, "album"),
        album_artist: None,
        date: field::<u16>(record, "year").map(|year| year.to_string()),
        genre: field(record, "genre"),
        track_number: field(record, "track"),
        track_total: None,
        disc: field(record, "discNumber"),
        composer: None,
        comment: None,
        lyrics: None,
    };
    let audio_format = AudioFormat {
        format: field(record, "suffix"),
        bitrate: field(record, "bitRate").map(Kbps),
        sample_rate: field(record, "samplingRate").map(Hertz),
        bits_per_sample: field(record, "bitDepth"),
        channels: field(record, "channelCount"),
        decibels: record
            .get("replayGain")
            .and_then(|replay_gain| field(replay_gain, "trackGain"))
            .map(Decibels),
    };
    let duration = Duration::from_secs(field(record, "duration").unwrap_or(0));
    Ok(Track::tagged(source, duration, tags).with_audio_format(audio_format))
}

fn id<'a>(server_name: &ServerName, record: &'a Value) -> Result<&'a str, RemoteError> {
    <&str>::deserialize(record.get("id").unwrap_or(&Value::Null))
        .map_err(|error| read_error(server_name, &error))
}

fn field<'a, T: Deserialize<'a>>(record: &'a Value, name: &str) -> Option<T> {
    record
        .get(name)
        .and_then(|value| T::deserialize(value).ok())
}

#[must_use]
pub fn stream_url(server_track_id: &ServerTrackId, session: &Session) -> String {
    format!(
        "{}/rest/stream?id={}&format=raw&{}",
        session.endpoint,
        encoded(server_track_id.as_str()),
        session.query
    )
}

pub fn download(
    agent: &Agent,
    media_fetch: &MediaFetch,
    media_dir: &Path,
) -> Result<Fetched, RemoteError> {
    let server_name = &media_fetch.server_name;
    let failed = |error: std::io::Error| cache_error(server_name, &error);
    let unreachable = |kind: ErrorKind| RemoteError::Unreachable {
        server_name: server_name.clone(),
        source: IoError::from(kind),
    };
    let first_byte = media_fetch.first_byte;
    let range = format!("bytes={first_byte}-{}", first_byte + FETCH_CHUNK - 1);
    let link = stream_url(&media_fetch.server_track_id, &media_fetch.session);
    let mut response = fetched(server_name, link, |current| {
        agent
            .get(current)
            .header("Range", &range)
            .header("Accept-Encoding", "identity")
            .config()
            .timeout_global(Some(FETCH_TIMEOUT))
            .build()
    })?;
    let protocol = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("json") || value.contains("xml"));
    if protocol {
        return answer(server_name, response).and_then(|_answer| {
            Err(RemoteError::Api {
                server_name: server_name.clone(),
                api_code: ApiCode(0),
            })
        });
    }
    let (start, total, limit) = span(&response, server_name)?;
    let part_path = media_dir.join(format!("{}.part", media_fetch.cache_key.as_str()));
    let mut reader = response.body_mut().as_reader().take(limit);
    let (downloaded, finished) =
        write_part(&part_path, start, &mut reader).map_err(failed)?;
    let byte_len = match (total, finished) {
        (Some(total), _) => total,
        (None, Ok(())) => downloaded,
        (None, Err(kind)) => return Err(unreachable(kind)),
    };
    if downloaded == start && downloaded < byte_len {
        return Err(unreachable(
            finished.err().unwrap_or(ErrorKind::UnexpectedEof),
        ));
    }
    if downloaded < byte_len {
        return Ok(Fetched {
            media_path: part_path,
            downloaded,
            byte_len,
        });
    }
    let media_path = media_dir.join(media_fetch.cache_key.as_str());
    fs::rename(&part_path, &media_path).map_err(failed)?;
    Ok(Fetched {
        media_path,
        downloaded,
        byte_len,
    })
}

fn span(
    response: &Response<Body>,
    server_name: &ServerName,
) -> Result<(u64, Option<u64>, u64), RemoteError> {
    let http_status = HttpStatus(response.status().as_u16());
    if http_status.0 != 206 {
        return Ok((0, response.body().content_length(), u64::MAX));
    }
    let (start, total) =
        content_range(response).ok_or_else(|| RemoteError::Status {
            server_name: server_name.clone(),
            http_status,
        })?;
    Ok((start, Some(total), FETCH_CHUNK))
}

fn write_part(
    part_path: &Path,
    start: u64,
    reader: &mut impl Read,
) -> std::io::Result<(u64, Result<(), ErrorKind>)> {
    if let Some(server_dir) = part_path.parent() {
        fs::create_dir_all(server_dir)?;
    }
    let mut part_file = fs::File::options()
        .write(true)
        .create(true)
        .truncate(start == 0)
        .open(part_path)?;
    part_file.seek(SeekFrom::Start(start))?;
    let finished = written(reader, &mut part_file)?;
    Ok((part_file.stream_position()?, finished))
}

fn written(
    reader: &mut impl Read,
    part_file: &mut fs::File,
) -> std::io::Result<Result<(), ErrorKind>> {
    let mut buffer = vec![0; READ_BYTES];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(Ok(())),
            Ok(read) => part_file.write_all(buffer.get(..read).unwrap_or(&[]))?,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Ok(Err(error.kind())),
        }
    }
}

fn content_range(response: &Response<Body>) -> Option<(u64, u64)> {
    let value = response.headers().get("content-range")?.to_str().ok()?;
    let (_unit, rest) = value.split_once(' ')?;
    let (span, total) = rest.split_once('/')?;
    let (start, _end) = span.split_once('-')?;
    Some((start.parse().ok()?, total.parse().ok()?))
}

pub(crate) fn cache_error(
    server_name: &ServerName,
    error: &std::io::Error,
) -> RemoteError {
    RemoteError::Cache {
        server_name: server_name.clone(),
        source: IoError::from(error.kind()),
    }
}

fn signed(account: &Account, secret: &Secret) -> Session {
    let salt = format!("{:012x}", fastrand::u64(..1 << 48));
    let token = format!("{:x}", md5::compute(format!("{}{salt}", secret.as_str())));
    Session::new(
        account.endpoint.clone(),
        &format!(
            "u={}&t={token}&s={salt}&v={API_VERSION}&c=sifr&f=json",
            encoded(account.user_name.as_str())
        ),
    )
}

fn encoded(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(crate) fn fetched(
    server_name: &ServerName,
    link: String,
    request: impl Fn(&str) -> RequestBuilder<WithoutBody>,
) -> Result<Response<Body>, RemoteError> {
    let mut current = link;
    for _ in 0..=MAX_REDIRECTS {
        let response =
            request(&current)
                .call()
                .map_err(|error| RemoteError::Unreachable {
                    server_name: server_name.clone(),
                    source: IoError::from(error.into_io().kind()),
                })?;
        let http_status = HttpStatus(response.status().as_u16());
        if !REDIRECT_STATUSES.contains(&http_status.0) {
            return if http_status.0 < 400 {
                Ok(response)
            } else {
                Err(RemoteError::Status {
                    server_name: server_name.clone(),
                    http_status,
                })
            };
        }
        current =
            redirected(&current, &response).ok_or_else(|| RemoteError::Moved {
                server_name: server_name.clone(),
            })?;
    }
    Err(RemoteError::Moved {
        server_name: server_name.clone(),
    })
}

fn redirected(current: &str, response: &Response<Body>) -> Option<String> {
    let location = response.headers().get("location")?.to_str().ok()?;
    let (base, query) = current.split_once('?').unwrap_or((current, ""));
    let (scheme, rest) = base.split_once("://")?;
    let authority = rest.split('/').next()?;
    let target = match location.strip_prefix('/') {
        Some(path) if path.starts_with('/') => format!("{scheme}:{location}"),
        Some(_path) => format!("{scheme}://{authority}{location}"),
        None => location.to_owned(),
    };
    let auth = query
        .rfind("&u=")
        .and_then(|at| query.get(at + 1..))
        .unwrap_or(query);
    let (target_base, target_query) = target.split_once('?').map_or_else(
        || (target.as_str(), query.to_owned()),
        |(target_base, own_query)| (target_base, format!("{own_query}&{auth}")),
    );
    let from = Endpoint::parse(base).ok()?;
    let to = Endpoint::parse(target_base).ok()?;
    let downgrade =
        from.as_str().starts_with("https://") && to.as_str().starts_with("http://");
    (from.host() == to.host() && !downgrade)
        .then(|| format!("{target_base}?{target_query}"))
}

fn read_error(server_name: &ServerName, error: &impl std::error::Error) -> RemoteError {
    RemoteError::Parse {
        server_name: server_name.clone(),
        diagnostic: Diagnostic::from_error(error),
    }
}
