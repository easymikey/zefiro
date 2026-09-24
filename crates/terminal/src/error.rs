use std::io;

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("terminal setup: {0}")]
    Setup(#[source] io::Error),
    #[error("terminal teardown: {0}")]
    Teardown(#[source] io::Error),
    #[error("a worker thread panicked: {report}")]
    WorkerPanic { report: String },
    #[error("{session} (teardown after also failed: {teardown})")]
    TeardownAfter {
        #[source]
        session: Box<TerminalError>,
        teardown: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::io;

    use rstest::rstest;

    use crate::error::TerminalError;

    #[rstest]
    #[case::setup(
        TerminalError::Setup(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "denied"
        )),
        "terminal setup: denied"
    )]
    #[case::teardown(
        TerminalError::Teardown(io::Error::other("broken pipe")),
        "terminal teardown: broken pipe"
    )]
    #[case::a_worker_thread_that_fell_over(
        TerminalError::WorkerPanic { report: String::from("panicked at src/input.rs:9:5: no tty") },
        "a worker thread panicked: panicked at src/input.rs:9:5: no tty"
    )]
    fn every_terminal_error_names_what_went_wrong(
        #[case] error: TerminalError,
        #[case] message: &str,
    ) {
        assert_eq!(error.to_string(), message);
    }
}
