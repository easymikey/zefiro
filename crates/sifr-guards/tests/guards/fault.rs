// GUARD: the one error type both config_doc guards fail through.

#[derive(Debug, thiserror::Error)]
pub(crate) enum GuardError {
    #[error("{said}")]
    Mismatch { said: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Toml(#[from] toml::de::Error),

    #[error(transparent)]
    Config(#[from] config::error::Error),
}

impl GuardError {
    pub(crate) fn mismatch(said: impl Into<String>) -> Self {
        Self::Mismatch { said: said.into() }
    }

    pub(crate) fn missing(said: impl Into<String>) -> Self {
        Self::Mismatch { said: said.into() }
    }
}
