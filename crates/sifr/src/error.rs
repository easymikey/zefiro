use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Runtime(#[from] runtime::error::Error),
    #[error(transparent)]
    Terminal(#[from] terminal::error::Error),
    #[error(transparent)]
    Library(#[from] library::error::Error),
    #[error("no config directory available")]
    ConfigDirUnset,
    #[error("no music directory configured and no default audio directory available")]
    MusicDirUnset,
    #[error("music directory not found: {path}")]
    MusicDirMissing { path: PathBuf },
    #[error("--playlist: {0}")]
    PlaylistName(#[source] kernel::domain::playlist::PlaylistNameError),
    #[error("--theme: {0}")]
    ThemeName(#[source] kernel::domain::theme::ThemeNameError),
    #[error("the embedded stock theme: {0}")]
    StockTheme(#[source] config::error::Error),
    #[error("installing signal handlers: {0}")]
    SignalHandlers(#[source] io::Error),
    #[error("the signal handler is already installed")]
    SignalHandlerInstalled,
    #[error("reading terminal input: {0}")]
    Input(#[source] io::Error),
    #[error("a background thread panicked")]
    WorkerPanicked,
    #[error("{run} (teardown after also failed: {teardown})")]
    RunAndTeardown {
        #[source]
        run: runtime::error::Error,
        teardown: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::error::Error;

    #[test]
    fn music_dir_unset_message_is_readable() {
        let error = Error::MusicDirUnset;

        assert_eq!(
            error.to_string(),
            "no music directory configured and no default audio directory available"
        );
    }

    #[test]
    fn run_and_teardown_message_names_both_errors() {
        let error = Error::RunAndTeardown {
            run: runtime::error::Error::InputClosed,
            teardown: io::Error::other("broken pipe"),
        };

        assert_eq!(
            error.to_string(),
            "input closed (teardown after also failed: broken pipe)"
        );
    }
}
