use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use kernel::domain::{
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
        RemoteError,
        Secret,
        SecretError,
        ServerName,
        Session,
    },
};
use keyring_core::Entry;
use ureq::Agent;

use crate::{
    http::CACHE_BYTES,
    message::RemoteMessage,
    subsonic::{
        answer,
        cache_error,
        catalog_rows,
        download,
        fetched,
        ping,
        query,
        search_query,
        search_rows,
    },
};

pub const KEYCHAIN_SERVICE: &str = "sifr";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteJob {
    Connect(Connection),
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
}

impl RemoteJob {
    #[must_use]
    pub fn run(self, agent: &Agent) -> RemoteMessage {
        match self {
            RemoteJob::Connect(connection) => connected(agent, &connection),
            RemoteJob::List {
                server_name,
                session,
                listing,
                page,
                revision,
            } => {
                let link = format!(
                    "{}/rest/{}&{}",
                    session.endpoint,
                    query(&listing, page),
                    session.query
                );
                let result = fetched(&server_name, link, |current| agent.get(current))
                    .and_then(|response| answer(&server_name, response))
                    .and_then(|answer| catalog_rows(&server_name, &listing, &answer));
                RemoteMessage::Listed {
                    server_name,
                    listing,
                    page,
                    result,
                    revision,
                }
            }
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
            } => {
                let link = search_query(&input, &session);
                RemoteMessage::Found {
                    result: fetched(&server_name, link, |current| agent.get(current))
                        .and_then(|response| answer(&server_name, response))
                        .and_then(|answer| search_rows(&server_name, &answer)),
                    server_name,
                    revision,
                }
            }
        }
    }

    pub(crate) fn revision(&self) -> Option<Revision> {
        match self {
            RemoteJob::Connect(_connection) => None,
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
        }
    }
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
