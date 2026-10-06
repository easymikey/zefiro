use std::{io, time::SystemTimeError};

use kernel::domain::{
    driver::{DriverError, DriverName},
    io_error::IoError,
};

#[derive(Debug, thiserror::Error)]
pub enum Error<E: std::error::Error + 'static = io::Error> {
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    #[error("input closed")]
    InputClosed,
    #[error("painting a frame: {0}")]
    Paint(#[source] E),
    #[error("spawning the event loop thread: {0}")]
    Host(#[source] io::Error),
    #[error("the event loop thread panicked")]
    EventLoopPanicked,
    #[error(transparent)]
    Clock(#[from] ClockError),
}

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("spawning the {driver_name} thread: {error}")]
    Thread {
        driver_name: DriverName,
        error: io::Error,
    },
    #[error("the {driver_name} driver stopped before handing over its tap")]
    TapLost { driver_name: DriverName },
}

#[derive(Debug, thiserror::Error)]
#[error("reading the system clock: {0}")]
pub struct ClockError(#[source] pub(crate) SystemTimeError);

impl From<&SpawnError> for DriverError {
    fn from(error: &SpawnError) -> Self {
        match error {
            SpawnError::Thread { error, .. } => DriverError::Spawn {
                error: error.kind().into(),
            },
            SpawnError::TapLost { .. } => DriverError::Spawn {
                error: IoError::Other,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use kernel::domain::{
        driver::{DriverError, DriverName},
        io_error::IoError,
    };

    use crate::error::{Error, SpawnError};

    #[test]
    fn spawn_message_includes_the_driver_and_the_error() {
        let error = SpawnError::Thread {
            driver_name: DriverName::Audio,
            error: io::Error::other("resource temporarily unavailable"),
        };

        assert_eq!(
            error.to_string(),
            "spawning the audio thread: resource temporarily unavailable"
        );
    }

    #[test]
    fn a_failed_spawn_reaches_the_kernel_as_spawn() {
        let error = SpawnError::Thread {
            driver_name: DriverName::Audio,
            error: io::Error::from(io::ErrorKind::PermissionDenied),
        };

        assert_eq!(
            DriverError::from(&error),
            DriverError::Spawn {
                error: IoError::Denied
            }
        );
    }

    #[test]
    fn a_lost_tap_reaches_the_kernel_as_spawn() {
        let error = SpawnError::TapLost {
            driver_name: DriverName::Audio,
        };

        assert_eq!(
            DriverError::from(&error),
            DriverError::Spawn {
                error: IoError::Other
            }
        );
    }

    #[test]
    fn input_closed_message_is_readable() {
        let error = Error::<io::Error>::InputClosed;

        assert_eq!(error.to_string(), "input closed");
    }

    #[test]
    fn paint_message_includes_the_shell_error() {
        let error = Error::Paint(io::Error::other("broken pipe"));

        assert_eq!(error.to_string(), "painting a frame: broken pipe");
    }
}
