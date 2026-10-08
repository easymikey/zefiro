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
    use rstest::rstest;

    use crate::error::{Error, SpawnError};

    #[rstest]
    #[case::spawn(
        Error::Spawn(SpawnError::Thread {
            driver_name: DriverName::Audio,
            error: io::Error::other("resource temporarily unavailable"),
        }),
        "spawning the audio thread: resource temporarily unavailable"
    )]
    #[case::input_closed(Error::InputClosed, "input closed")]
    #[case::paint(
        Error::Paint(io::Error::other("broken pipe")),
        "painting a frame: broken pipe"
    )]
    fn an_error_message_names_its_cause(#[case] error: Error, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::failed_spawn(
        SpawnError::Thread {
            driver_name: DriverName::Audio,
            error: io::Error::from(io::ErrorKind::PermissionDenied),
        },
        IoError::Denied
    )]
    #[case::lost_tap(
        SpawnError::TapLost {
            driver_name: DriverName::Audio,
        },
        IoError::Other
    )]
    fn a_spawn_error_reaches_the_kernel_as_spawn(
        #[case] error: SpawnError,
        #[case] expected: IoError,
    ) {
        assert_eq!(
            DriverError::from(&error),
            DriverError::Spawn { error: expected }
        );
    }
}
