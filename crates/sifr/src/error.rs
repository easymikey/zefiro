use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Runtime(#[from] runtime::RuntimeError),
    #[error(transparent)]
    Run(#[from] runtime::RunError<io::Error>),
    #[error(transparent)]
    Host(#[from] runtime::HostError),
    #[error(transparent)]
    Terminal(#[from] terminal::TerminalError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Library(#[from] library::LibraryError),
    #[error("reading {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: config::ConfigError,
    },
    #[error("no music directory configured and no default audio directory available")]
    MusicDirectoryUnset,
    #[error("music directory not found: {path}")]
    MusicDirectoryMissing { path: PathBuf },
    #[error("--playlist {name:?}: {source}")]
    PlaylistName {
        name: String,
        #[source]
        source: kernel::playlist::PlaylistNameRejection,
    },
    #[error(transparent)]
    SignalInstall(#[from] crate::signal::AlreadyInstalled),
    #[error("a background thread panicked: {report}")]
    WorkerPanic { report: String },
    #[error("{run} (teardown after also failed: {teardown})")]
    RunAndTeardown {
        #[source]
        run: runtime::RunError<io::Error>,
        teardown: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use crate::error::Error;

    #[test]
    fn music_directory_unset_message_is_readable() {
        let error = Error::MusicDirectoryUnset;

        assert_eq!(
            error.to_string(),
            "no music directory configured and no default audio directory available"
        );
    }

    #[test]
    fn config_read_message_includes_path_and_source() {
        let error = Error::ConfigRead {
            path: PathBuf::from("config.toml"),
            source: io::Error::other("permission denied"),
        };

        assert_eq!(error.to_string(), "reading config.toml: permission denied");
    }

    #[test]
    fn run_and_teardown_message_names_both_failures() {
        let error = Error::RunAndTeardown {
            run: runtime::RunError::InputClosed,
            teardown: io::Error::other("broken pipe"),
        };

        assert_eq!(
            error.to_string(),
            "input closed (teardown after also failed: broken pipe)"
        );
    }
}
