use std::{io, time::SystemTimeError};

use kernel::domain::driver::DriverName;

#[derive(Debug, thiserror::Error)]
pub enum Error<E: std::error::Error + 'static = io::Error> {
    #[error("spawning the {driver} thread: {source}")]
    Spawn {
        driver: DriverName,
        #[source]
        source: io::Error,
    },
    #[error("the {driver} driver stopped before handing over its tap")]
    TapLost { driver: DriverName },
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
#[error("reading the system clock: {0}")]
pub struct ClockError(#[source] pub(crate) SystemTimeError);

#[cfg(test)]
mod tests {
    use std::io;

    use kernel::domain::driver::DriverName;

    use crate::error::Error;

    #[test]
    fn spawn_message_includes_the_driver_and_the_source() {
        let error: Error = Error::Spawn {
            driver: DriverName::Audio,
            source: io::Error::other("resource temporarily unavailable"),
        };

        assert_eq!(
            error.to_string(),
            "spawning the audio thread: resource temporarily unavailable"
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
