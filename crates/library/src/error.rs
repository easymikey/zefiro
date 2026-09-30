use std::path::{Path, PathBuf};

use kernel::{IoError, LibraryError, LibrarySubject};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{subject} {path}: {source}")]
    Read {
        subject: LibrarySubject,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{subject} {path}: {source}")]
    Write {
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
    pub(crate) fn read(
        subject: LibrarySubject,
        path: &Path,
    ) -> impl FnOnce(std::io::Error) -> Error + use<> {
        let path = path.to_path_buf();
        move |source| Error::Read {
            subject,
            path,
            source,
        }
    }

    pub(crate) fn write(
        subject: LibrarySubject,
        path: &Path,
    ) -> impl FnOnce(std::io::Error) -> Error + use<> {
        let path = path.to_path_buf();
        move |source| Error::Write {
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

fn trash_error(source: &trash::Error) -> IoError {
    match source {
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
        match error {
            Error::Read {
                subject,
                path,
                source,
            }
            | Error::Write {
                subject,
                path,
                source,
            } => LibraryError::File {
                subject: *subject,
                path: path.clone(),
                kind: source.kind().into(),
            },
            Error::Json { subject, path, .. } => LibraryError::File {
                subject: *subject,
                path: path.clone(),
                kind: IoError::Malformed,
            },
            Error::Encode { path, .. } => LibraryError::File {
                subject: LibrarySubject::Cache,
                path: path.clone(),
                kind: IoError::Malformed,
            },
            Error::Trash { path, source } => LibraryError::File {
                subject: LibrarySubject::Trash,
                path: path.clone(),
                kind: trash_error(source),
            },
            Error::NoUserDirs => LibraryError::NoUserDirs,
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{IoError, LibraryError, LibrarySubject};
    use rstest::rstest;

    use crate::error::Error;

    #[rstest]
    #[case::no_directory(Error::NoUserDirs, "no such directory")]
    #[case::read(
        Error::Read {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "a scan /music: entity not found"
    )]
    #[case::write(
        Error::Write {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "the favorites file /data/favorites.json: entity not found"
    )]
    #[case::json(
        Error::Json {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            source: serde_json::from_str::<serde_json::Value>("")
                .expect_err("empty input must fail to parse"),
        },
        "the history file /data/history.jsonl: json: EOF while parsing a value at line 1 column 0"
    )]
    #[case::cache(
        Error::Encode {
            path: "/data/library.bin".into(),
            source: bincode::error::EncodeError::UnexpectedEnd,
        },
        "cache encode /data/library.bin: UnexpectedEnd"
    )]
    #[case::trash(
        Error::Trash {
            path: "/music/gone.flac".into(),
            source: trash::Error::Unknown {
                description: "no trash service".to_string(),
            },
        },
        "trash /music/gone.flac: Error during a `trash` operation: Unknown { description: \"no trash service\" }"
    )]
    fn errors_render_readable_messages(#[case] error: Error, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::read_missing(
        Error::Read {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        LibraryError::File {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            kind: IoError::Missing,
        }
    )]
    #[case::write_denied(
        Error::Write {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        LibraryError::File {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            kind: IoError::Denied,
        }
    )]
    #[case::json(
        Error::Json {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            source: serde_json::from_str::<serde_json::Value>("")
                .expect_err("empty input must fail to parse"),
        },
        LibraryError::File {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            kind: IoError::Malformed,
        }
    )]
    #[case::cache_encode(
        Error::Encode {
            path: "/data/library.bin".into(),
            source: bincode::error::EncodeError::UnexpectedEnd,
        },
        LibraryError::File {
            subject: LibrarySubject::Cache,
            path: "/data/library.bin".into(),
            kind: IoError::Malformed,
        }
    )]
    #[case::trash_unknown(
        Error::Trash {
            path: "/music/gone.flac".into(),
            source: trash::Error::Unknown {
                description: "no trash service".to_string(),
            },
        },
        LibraryError::File {
            subject: LibrarySubject::Trash,
            path: "/music/gone.flac".into(),
            kind: IoError::Other,
        }
    )]
    #[case::no_directory(Error::NoUserDirs, LibraryError::NoUserDirs)]
    fn a_library_error_becomes_a_structured_failure(
        #[case] error: Error,
        #[case] expected: LibraryError,
    ) {
        let failure: LibraryError = (&error).into();
        assert_eq!(failure, expected);
    }
}
