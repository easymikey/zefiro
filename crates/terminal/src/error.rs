use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("terminal setup: {0}")]
    Setup(#[source] io::Error),
    #[error("terminal teardown: {0}")]
    Teardown(#[source] io::Error),
    #[error("terminal setup: {setup_error}; teardown: {teardown_error}")]
    SetupAndTeardown {
        #[source]
        setup_error: io::Error,
        teardown_error: io::Error,
    },
    #[error("window colors: {0}")]
    WriteWindowColors(#[source] io::Error),
    #[error("terminal probe: {0}")]
    Query(#[source] ratatui_image::errors::Errors),
}

#[cfg(test)]
mod tests {
    use std::io;

    use rstest::rstest;

    use crate::error::Error;

    #[rstest]
    #[case::setup(
        Error::Setup(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        "terminal setup: denied"
    )]
    #[case::teardown(
        Error::Teardown(io::Error::other("broken pipe")),
        "terminal teardown: broken pipe"
    )]
    #[case::setup_and_teardown(
        Error::SetupAndTeardown {
            setup_error: io::Error::other("no tty"),
            teardown_error: io::Error::other("broken pipe"),
        },
        "terminal setup: no tty; teardown: broken pipe"
    )]
    #[case::window_colors(
        Error::WriteWindowColors(io::Error::other("closed")),
        "window colors: closed"
    )]
    #[case::probe(
        Error::Query(ratatui_image::errors::Errors::NoFontSize),
        "terminal probe: Could not detect font size"
    )]
    fn every_terminal_error_names_what_went_wrong(
        #[case] error: Error,
        #[case] message: &str,
    ) {
        assert_eq!(error.to_string(), message);
    }
}
