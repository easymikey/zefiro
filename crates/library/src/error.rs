use std::path::PathBuf;

use kernel::LibraryFailure;

#[derive(Debug, Clone, Copy)]
pub enum Subject {
    Scan,
    Playlist,
    History,
    Favorites,
    Cache,
}

impl std::fmt::Display for Subject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Subject::Scan => "scan",
            Subject::Playlist => "playlist",
            Subject::History => "history",
            Subject::Favorites => "favorites",
            Subject::Cache => "cache",
        };
        formatter.write_str(label)
    }
}

impl Subject {
    fn into_failure(self, reason: String) -> LibraryFailure {
        match self {
            Subject::Scan => LibraryFailure::Scan { reason },
            Subject::Playlist => LibraryFailure::Playlist { reason },
            Subject::History => LibraryFailure::History { reason },
            Subject::Favorites => LibraryFailure::Favorites { reason },
            Subject::Cache => LibraryFailure::Cache { reason },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("{subject} {path}: {source}")]
    Read {
        subject: Subject,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{subject} {path}: {source}")]
    Write {
        subject: Subject,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{subject} {path}: json: {source}")]
    Json {
        subject: Subject,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("cache encode: {0}")]
    Cache(#[source] bincode::error::EncodeError),

    #[error("trash {path}: {source}")]
    Trash {
        path: PathBuf,
        #[source]
        source: trash::Error,
    },

    #[error("no such directory")]
    NoDirectory,
}

impl From<&LibraryError> for LibraryFailure {
    fn from(error: &LibraryError) -> Self {
        let reason = error.to_string();
        match error {
            LibraryError::Read { subject, .. }
            | LibraryError::Write { subject, .. }
            | LibraryError::Json { subject, .. } => subject.into_failure(reason),
            LibraryError::Cache(_) => LibraryFailure::Cache { reason },
            LibraryError::Trash { .. } => LibraryFailure::Trash { reason },
            LibraryError::NoDirectory => LibraryFailure::NoDirectory,
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::LibraryFailure;
    use rstest::rstest;

    use crate::error::{LibraryError, Subject};

    #[rstest]
    #[case::no_directory(LibraryError::NoDirectory, "no such directory")]
    #[case::read(
        LibraryError::Read {
            subject: Subject::Scan,
            path: "/music".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "scan /music: entity not found"
    )]
    #[case::write(
        LibraryError::Write {
            subject: Subject::Favorites,
            path: "/data/favorites.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "favorites /data/favorites.json: entity not found"
    )]
    #[case::json(
        LibraryError::Json {
            subject: Subject::History,
            path: "/data/history.jsonl".into(),
            source: serde_json::from_str::<serde_json::Value>("")
                .expect_err("empty input must fail to parse"),
        },
        "history /data/history.jsonl: json: EOF while parsing a value at line 1 column 0"
    )]
    #[case::cache(
        LibraryError::Cache(bincode::error::EncodeError::UnexpectedEnd),
        "cache encode: UnexpectedEnd"
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
    #[case::scan(LibraryError::Read {
        subject: Subject::Scan,
        path: "/music".into(),
        source: std::io::Error::from(std::io::ErrorKind::NotFound),
    })]
    #[case::playlist(LibraryError::Read {
        subject: Subject::Playlist,
        path: "/playlists/My Mix.m3u8".into(),
        source: std::io::Error::from(std::io::ErrorKind::NotFound),
    })]
    #[case::cache(LibraryError::Cache(bincode::error::EncodeError::UnexpectedEnd))]
    #[case::trash(LibraryError::Trash {
        path: "/music/gone.flac".into(),
        source: trash::Error::Unknown {
            description: "no trash service".to_string(),
        },
    })]
    fn failures_keep_the_library_error_s_own_text(#[case] error: LibraryError) {
        let expected = error.to_string();
        let failure: LibraryFailure = (&error).into();
        assert_eq!(failure.to_string(), expected);
    }

    #[test]
    fn no_directory_maps_to_the_matching_failure() {
        let failure: LibraryFailure = (&LibraryError::NoDirectory).into();
        assert_eq!(failure, LibraryFailure::NoDirectory);
    }
}
