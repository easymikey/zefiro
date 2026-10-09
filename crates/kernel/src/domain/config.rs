use std::collections::HashMap;

use crate::domain::{io_error::IoError, theme::ThemeName};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConfigName {
    Config,
    Theme(ThemeName),
}

impl std::fmt::Display for ConfigName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigName::Config => formatter.write_str("the config file"),
            ConfigName::Theme(name) => write!(formatter, "the theme {name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{name} is unreadable: {error}")]
    Read { name: ConfigName, error: IoError },
    #[error("the themes folder is unreadable: {0}")]
    ListThemes(IoError),
    #[error("{name} could not be saved: {error}")]
    Save { name: ConfigName, error: IoError },
    #[error("Config watch failed: {0}")]
    Watch(IoError),
    #[error("{0}")]
    Parse(Diagnostic),
}

impl From<Diagnostic> for ConfigError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self::Parse(diagnostic)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic(String);

impl Diagnostic {
    #[must_use]
    pub fn from_error(error: &impl std::error::Error) -> Self {
        Self(error.to_string())
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ConfigErrors(pub(crate) HashMap<ConfigName, ConfigError>);

impl ConfigErrors {
    pub(crate) fn get(&self, name: &ConfigName) -> Option<&ConfigError> {
        self.0.get(name)
    }

    pub(crate) fn clear(&mut self, name: &ConfigName) -> Option<ConfigError> {
        self.0.remove(name)
    }
}

impl Extend<(ConfigName, ConfigError)> for ConfigErrors {
    fn extend<T: IntoIterator<Item = (ConfigName, ConfigError)>>(&mut self, errors: T) {
        self.0.extend(errors);
    }
}
