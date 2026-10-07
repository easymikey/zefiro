use std::path::{Path, PathBuf};

use kernel::{
    domain::io_error::IoError,
    message::{LibraryError, LibrarySubject},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{subject} {path}: {source}")]
    Io {
        subject: LibrarySubject,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{subject} {path}: json: {source}")]
    Json {
        subject: LibrarySubject,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("cache encode {path}: {source}")]
    Encode {
        path: PathBuf,
        #[source]
        source: bincode::error::EncodeError,
    },

    #[error("cache decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: bincode::error::DecodeError,
    },

    #[error("{} {path}: tags: {source}", LibrarySubject::Scan)]
    Tags {
        path: PathBuf,
        #[source]
        source: lofty::error::FileParseError,
    },

    #[error("trash {path}: {source}")]
    Trash {
        path: PathBuf,
        #[source]
        source: trash::Error,
    },

    #[error("no such directory")]
    NoUserDirs,
}

impl Error {
    pub(crate) fn io(
        subject: LibrarySubject,
        path: &Path,
    ) -> impl FnOnce(std::io::Error) -> Error + use<> {
        let path = path.to_path_buf();
        move |source| Error::Io {
            subject,
            path,
            source,
        }
    }

    pub(crate) fn json(
        subject: LibrarySubject,
        path: &Path,
    ) -> impl FnOnce(serde_json::Error) -> Error + use<> {
        let path = path.to_path_buf();
        move |source| Error::Json {
            subject,
            path,
            source,
        }
    }
}

fn trash_error(error: &trash::Error) -> IoError {
    match error {
        #[cfg(all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        ))]
        trash::Error::FileSystem { source, .. } => source.kind().into(),
        _ => IoError::Other,
    }
}

impl From<&Error> for LibraryError {
    fn from(error: &Error) -> Self {
        let (subject, path, io_error) = match error {
            Error::Io {
                subject,
                path,
                source,
            } => (*subject, path, source.kind().into()),
            Error::Json {
                subject,
                path,
                source: _source,
            } => (*subject, path, IoError::Malformed),
            Error::Encode {
                path,
                source: _source,
            } => (LibrarySubject::Cache, path, IoError::Malformed),
            Error::Decode {
                path,
                source: _source,
            } => (LibrarySubject::Cache, path, IoError::Malformed),
            Error::Tags {
                path,
                source: _source,
            } => (LibrarySubject::Scan, path, IoError::Malformed),
            Error::Trash { path, source } => {
                (LibrarySubject::Trash, path, trash_error(source))
            }
            Error::NoUserDirs => return LibraryError::NoUserDirs,
        };
        LibraryError::Disk {
            subject,
            path: path.clone(),
            error: io_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::io_error::IoError,
        message::{LibraryError, LibrarySubject},
    };
    use rstest::rstest;

    use crate::error::Error;

    fn file(subject: LibrarySubject, path: &str, error: IoError) -> LibraryError {
        LibraryError::Disk {
            subject,
            path: path.into(),
            error,
        }
    }

    #[rstest]
    #[case::no_user_dirs(
        Error::NoUserDirs,
        "no such directory",
        LibraryError::NoUserDirs
    )]
    #[case::read_missing(
        Error::Io {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "a scan /music: entity not found",
        file(LibrarySubject::Scan, "/music", IoError::Missing)
    )]
    #[case::write_denied(
        Error::Io {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        "the favorites file /data/favorites.json: permission denied",
        file(LibrarySubject::Favorites, "/data/favorites.json", IoError::Denied)
    )]
    #[case::json(
        Error::Json {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            source: serde_json::from_str::<serde_json::Value>("")
                .expect_err("empty input must fail to parse"),
        },
        "the history file /data/history.jsonl: json: EOF while parsing a value at line 1 column 0",
        file(LibrarySubject::History, "/data/history.jsonl", IoError::Malformed)
    )]
    #[case::cache_encode(
        Error::Encode {
            path: "/data/library.bin".into(),
            source: bincode::error::EncodeError::UnexpectedEnd,
        },
        "cache encode /data/library.bin: UnexpectedEnd",
        file(LibrarySubject::Cache, "/data/library.bin", IoError::Malformed)
    )]
    #[case::trash_unknown(
        Error::Trash {
            path: "/music/gone.flac".into(),
            source: trash::Error::Unknown {
                description: "no trash service".to_string(),
            },
        },
        "trash /music/gone.flac: Error during a `trash` operation: Unknown { description: \"no trash service\" }",
        file(LibrarySubject::Trash, "/music/gone.flac", IoError::Other)
    )]
    fn errors_render_messages_and_structured_errors(
        #[case] error: Error,
        #[case] message: &str,
        #[case] library_error: LibraryError,
    ) {
        assert_eq!(error.to_string(), message);
        assert_eq!(LibraryError::from(&error), library_error);
    }

    #[rstest]
    #[case::not_found(std::io::ErrorKind::NotFound, IoError::Missing)]
    #[case::permission_denied(std::io::ErrorKind::PermissionDenied, IoError::Denied)]
    #[case::storage_full(std::io::ErrorKind::StorageFull, IoError::Full)]
    #[case::other(std::io::ErrorKind::Interrupted, IoError::Other)]
    fn io_error_maps_kinds(
        #[case] kind: std::io::ErrorKind,
        #[case] expected: IoError,
    ) {
        assert_eq!(IoError::from(kind), expected);
    }
}
