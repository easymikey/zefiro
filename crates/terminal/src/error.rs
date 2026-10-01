use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("terminal setup: {0}")]
    Setup(#[source] io::Error),
    #[error("terminal teardown: {0}")]
    Teardown(#[source] io::Error),
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
    fn every_terminal_error_names_what_went_wrong(
        #[case] error: Error,
        #[case] message: &str,
    ) {
        assert_eq!(error.to_string(), message);
    }
}
