use std::collections::HashMap;

use crate::domain::{io_error::IoError, theme::ThemeName};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConfigName {
    Config,
    Appearance,
    Theme(ThemeName),
}

impl std::fmt::Display for ConfigName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ConfigName::Appearance => "the appearance file",
            ConfigName::Config => "the config file",
            ConfigName::Theme(name) => return write!(formatter, "the theme {name}"),
        };
        formatter.write_str(name)
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
pub(crate) struct ConfigErrors(HashMap<ConfigName, ConfigError>);

impl ConfigErrors {
    pub(crate) fn insert_if_changed(
        &mut self,
        name: ConfigName,
        error: ConfigError,
    ) -> bool {
        let is_unchanged = self.0.get(&name) == Some(&error);
        self.0.insert(name, error);
        !is_unchanged
    }

    pub(crate) fn get(&self, name: &ConfigName) -> Option<&ConfigError> {
        self.0.get(name)
    }

    pub(crate) fn clear(&mut self, name: &ConfigName) -> Option<ConfigError> {
        self.0.remove(name)
    }
}
