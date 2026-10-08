use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use kernel::domain::{
    favorites::Favorite,
    io_error::IoError,
    revision::Revision,
    server::{
        Account,
        CacheKey,
        Connection,
        Credential,
        Fetched,
        Listing,
        MediaFetch,
        Page,
        PlayReport,
        RemoteError,
        Scrobble,
        Secret,
        SecretError,
        ServerName,
        ServerTrackId,
        Session,
    },
    time::Moment,
};
use keyring_core::Entry;
use ureq::Agent;

use crate::{
    http::CACHE_BYTES,
    message::RemoteMessage,
    subsonic::{
        cache_error,
        catalog_rows,
        download,
        get,
        ping,
        query,
        scrobble,
        search_query,
        search_rows,
        star,
    },
};

pub const KEYCHAIN_SERVICE: &str = "sifr";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteJob {
    Connect(Connection),
    Forget(Account),
    List {
        server_name: ServerName,
        session: Session,
        listing: Listing,
        page: Page,
        revision: Revision,
    },
    Fetch {
        media_fetch: MediaFetch,
        media_dir: Arc<Path>,
        kept_cache_keys: Vec<CacheKey>,
    },
    Prefetch {
        media_fetch: MediaFetch,
        media_dir: Arc<Path>,
        kept_cache_keys: Vec<CacheKey>,
    },
    Search {
        server_name: ServerName,
        session: Session,
        input: String,
        revision: Revision,
    },
    Star {
        server_name: ServerName,
        session: Session,
        server_track_id: ServerTrackId,
        favorite: Favorite,
    },
    Report(Vec<SignedReport>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedReport {
    pub session: Session,
    pub play_report: PlayReport,
}

impl RemoteJob {
    #[must_use]
    pub fn run(self, agent: &Agent) -> RemoteMessage {
        match self {
            RemoteJob::Connect(connection) => connected(agent, &connection),
            RemoteJob::Forget(account) => RemoteMessage::Forgotten(forget(&account)),
            RemoteJob::List {
                server_name,
                session,
                listing,
                page,
                revision,
            } => RemoteMessage::Listed {
                result: get(agent, &server_name, query(&listing, page, &session))
                    .and_then(|answer| catalog_rows(&server_name, &listing, &answer)),
                server_name,
                listing,
                page,
                revision,
            },
            RemoteJob::Fetch {
                media_fetch,
                media_dir,
                kept_cache_keys,
            }
            | RemoteJob::Prefetch {
                media_fetch,
                media_dir,
                kept_cache_keys,
            } => RemoteMessage::Fetched {
                revision: media_fetch.revision,
                result: cached(&media_fetch, &media_dir).unwrap_or_else(|| {
                    evict(&media_fetch, &media_dir, &kept_cache_keys)
                        .and_then(|()| download(agent, &media_fetch, &media_dir))
                }),
            },
            RemoteJob::Search {
                server_name,
                session,
                input,
                revision,
            } => RemoteMessage::Found {
                result: get(agent, &server_name, search_query(&input, &session))
                    .and_then(|answer| search_rows(&server_name, &answer)),
                server_name,
                revision,
            },
            RemoteJob::Star {
                server_name,
                session,
                server_track_id: id,
                favorite,
            } => RemoteMessage::Starred {
                result: sent(agent, &server_name, star(&id, favorite, &session)),
                server_name,
                server_track_id: id,
                favorite,
            },
            RemoteJob::Report(signed_reports) => reported(agent, signed_reports),
        }
    }

    pub(crate) fn revision(&self) -> Option<Revision> {
        match self {
            RemoteJob::Connect(_connection) => None,
            RemoteJob::Forget(_account) => None,
            RemoteJob::List {
                server_name: _server_name,
                session: _session,
                listing: _listing,
                page: _page,
                revision,
            } => Some(*revision),
            RemoteJob::Fetch {
                media_fetch: _media_fetch,
                media_dir: _media_dir,
                kept_cache_keys: _kept_cache_keys,
            }
            | RemoteJob::Prefetch {
                media_fetch: _media_fetch,
                media_dir: _media_dir,
                kept_cache_keys: _kept_cache_keys,
            } => None,
            RemoteJob::Search {
                server_name: _server_name,
                session: _session,
                input: _input,
                revision,
            } => Some(*revision),
            RemoteJob::Star {
                server_name: _server_name,
                session: _session,
                server_track_id: _server_track_id,
                favorite: _favorite,
            } => None,
            RemoteJob::Report(_signed_reports) => None,
        }
    }

    pub(crate) fn server_name(&self) -> Option<&ServerName> {
        match self {
            RemoteJob::Connect(connection) => Some(&connection.account.server_name),
            RemoteJob::Forget(account) => Some(&account.server_name),
            RemoteJob::List {
                server_name,
                session: _session,
                listing: _listing,
                page: _page,
                revision: _revision,
            } => Some(server_name),
            RemoteJob::Search {
                server_name,
                session: _session,
                input: _input,
                revision: _revision,
            } => Some(server_name),
            RemoteJob::Star {
                server_name,
                session: _session,
                server_track_id: _server_track_id,
                favorite: _favorite,
            } => Some(server_name),
            RemoteJob::Fetch {
                media_fetch,
                media_dir: _media_dir,
                kept_cache_keys: _kept_cache_keys,
            }
            | RemoteJob::Prefetch {
                media_fetch,
                media_dir: _media_dir,
                kept_cache_keys: _kept_cache_keys,
            } => Some(&media_fetch.server_name),
            RemoteJob::Report(_signed_reports) => None,
        }
    }
}

fn reported(agent: &Agent, signed_reports: Vec<SignedReport>) -> RemoteMessage {
    let mut play_reports = Vec::with_capacity(signed_reports.len());
    let result = signed_reports.into_iter().try_for_each(
        |SignedReport {
             session,
             play_report,
         }| {
            let link = scrobble(&play_report, &session);
            sent(agent, &play_report.server_name, link)
                .map(|()| play_reports.push(play_report))
        },
    );
    RemoteMessage::Reported {
        play_reports,
        result,
    }
}

pub(crate) fn flush(
    reports_path: &Path,
    play_reports: &[PlayReport],
) -> Result<(), IoError> {
    let stored: Vec<(&str, &str, Duration)> = play_reports
        .iter()
        .filter_map(|play_report| match play_report.scrobble {
            Scrobble::NowPlaying => None,
            Scrobble::Played(moment) => Some((
                play_report.server_name.as_str(),
                play_report.server_track_id.as_str(),
                moment.since_epoch(),
            )),
        })
        .collect();
    serde_json::to_vec(&stored)
        .map_err(io::Error::from)
        .and_then(|bytes| {
            if let Some(reports_dir) = reports_path.parent() {
                fs::create_dir_all(reports_dir)?;
            }
            fs::write(reports_path, bytes)
        })
        .map_err(|error| IoError::from(error.kind()))
}

pub(crate) fn restored(reports_path: &Path) -> Result<Vec<PlayReport>, IoError> {
    let bytes = match fs::read(reports_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(IoError::from(error.kind())),
    };
    let stored: Vec<(String, String, Duration)> =
        serde_json::from_slice(&bytes).map_err(|_error| IoError::Malformed)?;
    Ok(stored
        .into_iter()
        .map(|(server_name, server_track_id, since_epoch)| PlayReport {
            server_name: ServerName::new(&server_name),
            server_track_id: ServerTrackId::new(&server_track_id),
            scrobble: Scrobble::Played(Moment::new(since_epoch)),
        })
        .collect())
}

fn sent(
    agent: &Agent,
    server_name: &ServerName,
    link: String,
) -> Result<(), RemoteError> {
    get(agent, server_name, link).map(drop)
}

fn cached(
    media_fetch: &MediaFetch,
    media_dir: &Path,
) -> Option<Result<Fetched, RemoteError>> {
    let media_path = media_dir.join(media_fetch.cache_key.as_str());
    let byte_len = fs::metadata(&media_path).ok()?.len();
    Some(
        fs::File::options()
            .write(true)
            .open(&media_path)
            .and_then(|file| file.set_modified(SystemTime::now()))
            .map(|()| Fetched {
                media_path,
                downloaded: byte_len,
                byte_len,
            })
            .map_err(|error| cache_error(&media_fetch.server_name, &error)),
    )
}

fn evict(
    media_fetch: &MediaFetch,
    media_dir: &Path,
    kept_cache_keys: &[CacheKey],
) -> Result<(), RemoteError> {
    if media_fetch.first_byte > 0 {
        return Ok(());
    }
    let failed = |error: io::Error| cache_error(&media_fetch.server_name, &error);
    let mut files = match cached_files(media_dir) {
        Ok(files) => files,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failed(error)),
    };
    files.sort();
    let kept_paths: Vec<PathBuf> = kept_cache_keys
        .iter()
        .flat_map(|cache_key| {
            [
                media_dir.join(cache_key.as_str()),
                media_dir.join(format!("{}.part", cache_key.as_str())),
            ]
        })
        .collect();
    let mut byte_len: u64 = files.iter().map(|(_modified, len, _path)| len).sum();
    for (_modified, len, path) in files {
        if byte_len < CACHE_BYTES {
            break;
        }
        if kept_paths.contains(&path) {
            continue;
        }
        if let Err(error) = fs::remove_file(&path)
            && error.kind() != ErrorKind::NotFound
        {
            return Err(failed(error));
        }
        byte_len -= len;
    }
    Ok(())
}

fn cached_files(media_dir: &Path) -> io::Result<Vec<(SystemTime, u64, PathBuf)>> {
    let mut files = Vec::new();
    for server_dir in fs::read_dir(media_dir)? {
        let server_dir = server_dir?.path();
        if !server_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&server_dir)? {
            let path = entry?.path();
            match fs::metadata(&path) {
                Ok(metadata) => {
                    files.push((metadata.modified()?, metadata.len(), path));
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(files)
}

fn connected(agent: &Agent, connection: &Connection) -> RemoteMessage {
    let account = &connection.account;
    let (result, stored) = match &connection.credential {
        Credential::Stored => (
            password(account).and_then(|secret| ping(agent, connection, &secret)),
            Ok(()),
        ),
        Credential::Typed(secret) => {
            let result = ping(agent, connection, secret);
            let stored = match &result {
                Ok(_session) => store(account, secret),
                Err(_error) => Ok(()),
            };
            (result, stored)
        }
    };
    RemoteMessage::Connected {
        server_name: account.server_name.clone(),
        result,
        stored,
    }
}

fn password(account: &Account) -> Result<Secret, RemoteError> {
    let server_name = &account.server_name;
    let no_password = || RemoteError::NoPassword {
        server_name: server_name.clone(),
    };
    let password = entry(account)?.get_password().map_err(|error| {
        if matches!(error, keyring_core::Error::NoEntry) {
            no_password()
        } else {
            keychain(server_name, &error)
        }
    })?;
    Secret::new(&password).map_err(|SecretError::Empty| no_password())
}

fn store(account: &Account, secret: &Secret) -> Result<(), RemoteError> {
    entry(account)?
        .set_password(secret.as_str())
        .map_err(|error| keychain(&account.server_name, &error))
}

fn forget(account: &Account) -> Result<(), RemoteError> {
    entry(account)?.delete_credential().or_else(|error| {
        if matches!(error, keyring_core::Error::NoEntry) {
            Ok(())
        } else {
            Err(keychain(&account.server_name, &error))
        }
    })
}

fn entry(account: &Account) -> Result<Entry, RemoteError> {
    Entry::new(KEYCHAIN_SERVICE, &account.keychain_account())
        .map_err(|error| keychain(&account.server_name, &error))
}

fn keychain(server_name: &ServerName, error: &keyring_core::Error) -> RemoteError {
    RemoteError::Keychain {
        server_name: server_name.clone(),
        source: IoError::from(
            if matches!(error, keyring_core::Error::NoStorageAccess(_)) {
                ErrorKind::PermissionDenied
            } else {
                ErrorKind::Other
            },
        ),
    }
}
