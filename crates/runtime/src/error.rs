use std::{error::Error, io, path::PathBuf};

use kernel::domain::Driver;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveError {
    #[error("no config directory available")]
    NoConfigDirectory,
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: config::ConfigError,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("spawning the {driver} thread: {source}")]
    Spawn {
        driver: Driver,
        #[source]
        source: io::Error,
    },
    #[error("the audio launcher produced no spectrum tap")]
    NoSpectrum,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("spawning the event loop thread: {0}")]
    Spawn(#[source] io::Error),
    #[error("the event loop thread panicked")]
    EventLoopPanicked,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError<E: Error + 'static> {
    #[error("input closed")]
    InputClosed,
    #[error("painting a frame: {0}")]
    Paint(#[source] E),
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use kernel::domain::Driver;

    use crate::error::{RunError, RuntimeError, SaveError};

    #[test]
    fn no_config_dir_message_is_readable() {
        let error = SaveError::NoConfigDirectory;

        assert_eq!(error.to_string(), "no config directory available");
    }

    #[test]
    fn read_message_includes_path_and_source() {
        let error = SaveError::Read {
            path: PathBuf::from("config.toml"),
            source: io::Error::other("permission denied"),
        };

        assert_eq!(error.to_string(), "reading config.toml: permission denied");
    }

    #[test]
    fn write_message_includes_path_and_source() {
        let error = SaveError::Write {
            path: PathBuf::from("config.toml"),
            source: io::Error::other("disk full"),
        };

        assert_eq!(error.to_string(), "writing config.toml: disk full");
    }

    #[test]
    fn spawn_message_includes_the_driver_and_the_source() {
        let error = RuntimeError::Spawn {
            driver: Driver::Audio,
            source: io::Error::other("resource temporarily unavailable"),
        };

        assert_eq!(
            error.to_string(),
            "spawning the audio thread: resource temporarily unavailable"
        );
    }

    #[test]
    fn input_closed_message_is_readable() {
        let error = RunError::<io::Error>::InputClosed;

        assert_eq!(error.to_string(), "input closed");
    }

    #[test]
    fn paint_message_includes_the_shell_error() {
        let error = RunError::Paint(io::Error::other("broken pipe"));

        assert_eq!(error.to_string(), "painting a frame: broken pipe");
    }
}
