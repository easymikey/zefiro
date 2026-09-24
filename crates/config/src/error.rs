use std::num::ParseIntError;

use kernel::domain::{CrossfadeOutOfRange, SettingId};
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{message}\n{file}:{line}")]
    Parse {
        file: String,
        line: usize,
        message: String,
    },
    #[error("`{key}` is not a table")]
    NotATable { key: String },
    #[error("{0}")]
    Document(#[from] toml_edit::TomlError),
    #[error(transparent)]
    Crossfade(#[from] CrossfadeRejection),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CrossfadeRejection {
    #[error("invalid crossfade {value:?}: {source}")]
    Number {
        value: String,
        #[source]
        source: ParseIntError,
    },
    #[error(
        "invalid crossfade {value:?}: expected an integer with an 's' or 'ms' suffix"
    )]
    MissingSuffix { value: String },
    #[error(transparent)]
    OutOfRange(#[from] CrossfadeOutOfRange),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid color `{input}`: expected 6 hex digits as #rrggbb")]
pub struct ColorRejection {
    pub input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingRejection {
    #[error("no appearance row carries the id {}", .id.0)]
    UnknownRow { id: SettingId },
    #[error("appearance row {} has no option at position {position}", .id.0)]
    NoOption { id: SettingId, position: usize },
}

fn line_at(source: &str, offset: usize) -> usize {
    source
        .get(..offset)
        .unwrap_or(source)
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn first_line(message: &str) -> String {
    message.lines().next().unwrap_or(message).trim().to_owned()
}

pub(crate) fn named_toml<T>(source: &str, file: &str) -> Result<T, ConfigError>
where
    T: DeserializeOwned,
{
    toml::from_str(source).map_err(|error| ConfigError::Parse {
        file: file.to_owned(),
        line: line_at(source, error.span().map_or(0, |span| span.start)),
        message: first_line(error.message()),
    })
}

#[cfg(test)]
mod tests {
    use crate::error::{ConfigError, named_toml};

    fn fault_text(source: &str, file: &str) -> String {
        let parsed: Result<toml::Table, ConfigError> = named_toml(source, file);
        parsed
            .err()
            .map(|fault| fault.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn a_fault_puts_the_message_first_and_the_place_below_it() {
        let text = fault_text("a = 1\n\n[card]\nb = 2\n[card]\n", "sifr-ui.toml");

        let place = text.lines().nth(1);

        assert_eq!(place, Some("sifr-ui.toml:5"), "whole text was {text:?}");
    }

    #[test]
    fn a_fault_on_the_first_line_reports_line_one_in_exactly_two_lines() {
        let text = fault_text("[card\n", "config.toml");

        assert_eq!(text.lines().nth(1), Some("config.toml:1"));
        assert_eq!(text.lines().count(), 2, "whole text was {text:?}");
    }
}
