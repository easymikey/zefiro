use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Runtime(#[from] runtime::Error),
    #[error(transparent)]
    Terminal(#[from] terminal::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Library(#[from] library::Error),
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
        source: config::Error,
    },
    #[error("no music directory configured and no default audio directory available")]
    MusicDirectoryUnset,
    #[error("music directory not found: {path}")]
    MusicDirectoryMissing { path: PathBuf },
    #[error("--playlist {name:?}: {source}")]
    PlaylistName {
        name: String,
        #[source]
        source: kernel::playlist::PlaylistNameError,
    },
    #[error(transparent)]
    SignalInstall(#[from] crate::signal::AlreadyInstalled),
    #[error("a background thread panicked: {report}")]
    WorkerPanic { report: String },
    #[error("{run} (teardown after also failed: {teardown})")]
    RunAndTeardown {
        #[source]
        run: runtime::Error,
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
            run: runtime::Error::InputClosed,
            teardown: io::Error::other("broken pipe"),
        };

        assert_eq!(
            error.to_string(),
            "input closed (teardown after also failed: broken pipe)"
        );
    }
}
