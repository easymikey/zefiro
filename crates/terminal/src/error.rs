use std::io;

use crate::window_colors::UnknownThemeError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("terminal setup: {0}")]
    Setup(#[source] io::Error),
    #[error("terminal teardown: {0}")]
    Teardown(#[source] io::Error),
    #[error("window colors: {0}")]
    UnknownTheme(#[source] UnknownThemeError),
    #[error("window colors: {0}")]
    WindowColors(#[source] io::Error),
    #[error("terminal probe: {0}")]
    Probe(#[source] ratatui_image::errors::Errors),
}

#[cfg(test)]
mod tests {
    use std::io;

    use kernel::domain::ThemeName;
    use rstest::rstest;

    use crate::{error::Error, window_colors::UnknownThemeError};

    #[rstest]
    #[case::setup(
        Error::Setup(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        "terminal setup: denied"
    )]
    #[case::teardown(
        Error::Teardown(io::Error::other("broken pipe")),
        "terminal teardown: broken pipe"
    )]
    #[case::unknown_theme(
        Error::UnknownTheme(UnknownThemeError { name: ThemeName::from_static("ember") }),
        "window colors: no such theme: ember"
    )]
    #[case::window_colors(
        Error::WindowColors(io::Error::other("closed")),
        "window colors: closed"
    )]
    #[case::probe(
        Error::Probe(ratatui_image::errors::Errors::NoFontSize),
        "terminal probe: Could not detect font size"
    )]
    fn every_terminal_error_names_what_went_wrong(
        #[case] error: Error,
        #[case] message: &str,
    ) {
        assert_eq!(error.to_string(), message);
    }
}
