use std::path::PathBuf;

use kernel::{IoFault, LibraryFailure, LibrarySubject};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
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
    Cache {
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
    NoDirectory,
}

fn trash_fault(source: &trash::Error) -> IoFault {
    match source {
        #[cfg(all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        ))]
        trash::Error::FileSystem { source, .. } => source.kind().into(),
        _ => IoFault::Other,
    }
}

impl From<&LibraryError> for LibraryFailure {
    fn from(error: &LibraryError) -> Self {
        match error {
            LibraryError::Read {
                subject,
                path,
                source,
            }
            | LibraryError::Write {
                subject,
                path,
                source,
            } => LibraryFailure::File {
                subject: *subject,
                path: path.clone(),
                fault: source.kind().into(),
            },
            LibraryError::Json { subject, path, .. } => LibraryFailure::File {
                subject: *subject,
                path: path.clone(),
                fault: IoFault::Malformed,
            },
            LibraryError::Cache { path, .. } => LibraryFailure::File {
                subject: LibrarySubject::Cache,
                path: path.clone(),
                fault: IoFault::Malformed,
            },
            LibraryError::Trash { path, source } => LibraryFailure::File {
                subject: LibrarySubject::Trash,
                path: path.clone(),
                fault: trash_fault(source),
            },
            LibraryError::NoDirectory => LibraryFailure::NoDirectory,
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{IoFault, LibraryFailure, LibrarySubject};
    use rstest::rstest;

    use crate::error::LibraryError;

    #[rstest]
    #[case::no_directory(LibraryError::NoDirectory, "no such directory")]
    #[case::read(
        LibraryError::Read {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "a scan /music: entity not found"
    )]
    #[case::write(
        LibraryError::Write {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "the favorites file /data/favorites.json: entity not found"
    )]
    #[case::json(
        LibraryError::Json {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            source: serde_json::from_str::<serde_json::Value>("")
                .expect_err("empty input must fail to parse"),
        },
        "the history file /data/history.jsonl: json: EOF while parsing a value at line 1 column 0"
    )]
    #[case::cache(
        LibraryError::Cache {
            path: "/data/library.bin".into(),
            source: bincode::error::EncodeError::UnexpectedEnd,
        },
        "cache encode /data/library.bin: UnexpectedEnd"
    )]
    #[case::trash(
        LibraryError::Trash {
            path: "/music/gone.flac".into(),
            source: trash::Error::Unknown {
                description: "no trash service".to_string(),
            },
        },
        "trash /music/gone.flac: Error during a `trash` operation: Unknown { description: \"no trash service\" }"
    )]
    fn errors_render_readable_messages(
        #[case] error: LibraryError,
        #[case] expected: &str,
    ) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::read_missing(
        LibraryError::Read {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        LibraryFailure::File {
            subject: LibrarySubject::Scan,
            path: "/music".into(),
            fault: IoFault::Missing,
        }
    )]
    #[case::write_denied(
        LibraryError::Write {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        LibraryFailure::File {
            subject: LibrarySubject::Favorites,
            path: "/data/favorites.json".into(),
            fault: IoFault::Denied,
        }
    )]
    #[case::json(
        LibraryError::Json {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            source: serde_json::from_str::<serde_json::Value>("")
                .expect_err("empty input must fail to parse"),
        },
        LibraryFailure::File {
            subject: LibrarySubject::History,
            path: "/data/history.jsonl".into(),
            fault: IoFault::Malformed,
        }
    )]
    #[case::cache_encode(
        LibraryError::Cache {
            path: "/data/library.bin".into(),
            source: bincode::error::EncodeError::UnexpectedEnd,
        },
        LibraryFailure::File {
            subject: LibrarySubject::Cache,
            path: "/data/library.bin".into(),
            fault: IoFault::Malformed,
        }
    )]
    #[case::trash_unknown(
        LibraryError::Trash {
            path: "/music/gone.flac".into(),
            source: trash::Error::Unknown {
                description: "no trash service".to_string(),
            },
        },
        LibraryFailure::File {
            subject: LibrarySubject::Trash,
            path: "/music/gone.flac".into(),
            fault: IoFault::Other,
        }
    )]
    #[case::no_directory(LibraryError::NoDirectory, LibraryFailure::NoDirectory)]
    fn a_library_error_becomes_a_structured_failure(
        #[case] error: LibraryError,
        #[case] expected: LibraryFailure,
    ) {
        let failure: LibraryFailure = (&error).into();
        assert_eq!(failure, expected);
    }
}
