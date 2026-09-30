use std::{fmt, num::ParseIntError};

use kernel::domain::{CrossfadeOutOfRange, OptionIndex, SettingId};
use serde::de::DeserializeOwned;

use crate::{
    appearance_file::APPEARANCE_FILE_NAME,
    config_file::CONFIG_FILE_NAME,
    theme_file::theme_file_name,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlFile {
    Config,
    Appearance,
    Theme(String),
}

impl fmt::Display for TomlFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TomlFile::Config => formatter.write_str(CONFIG_FILE_NAME),
            TomlFile::Appearance => formatter.write_str(APPEARANCE_FILE_NAME),
            TomlFile::Theme(name) => formatter.write_str(&theme_file_name(name)),
        }
    }
}

fn first_line(error: &toml::de::Error) -> &str {
    let message = error.message();
    message.lines().next().unwrap_or(message).trim()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{}\n{file}:{line}", first_line(.source))]
    Parse {
        file: TomlFile,
        line: usize,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("`{key}` is not a table")]
    NotATable { key: String },
    #[error("{}", .0.message())]
    Document(#[from] toml_edit::TomlError),
    #[error(transparent)]
    Crossfade(#[from] CrossfadeError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CrossfadeError {
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
pub struct ColorError {
    pub input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingError {
    #[error("no appearance row carries the id {}", .id.get())]
    UnknownRow { id: SettingId },
    #[error("appearance row {} has no option at position {}", .id.get(), .option.get())]
    NoOption { id: SettingId, option: OptionIndex },
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

pub(crate) fn parse_toml<T>(source: &str, file: TomlFile) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    toml::from_str(source).map_err(|error| Error::Parse {
        file,
        line: line_at(source, error.span().map_or(0, |span| span.start)),
        source: Box::new(error),
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::error::{Error, TomlFile, parse_toml};

    fn error_text(source: &str, file: TomlFile) -> String {
        let parsed: Result<toml::Table, Error> = parse_toml(source, file);
        parsed
            .err()
            .map_or_else(String::new, |error| error.to_string())
    }

    #[test]
    fn an_error_puts_the_message_first_and_the_place_below_it() {
        let text = error_text("a = 1\n\n[card]\nb = 2\n[card]\n", TomlFile::Appearance);

        let place = text.lines().nth(1);

        assert_eq!(place, Some("sifr-ui.toml:5"), "whole text was {text:?}");
    }

    #[test]
    fn an_error_on_the_first_line_reports_line_one_in_exactly_two_lines() {
        let text = error_text("[card\n", TomlFile::Config);

        assert_eq!(text.lines().nth(1), Some("config.toml:1"));
        assert_eq!(text.lines().count(), 2, "whole text was {text:?}");
    }

    #[rstest]
    #[case::config(TomlFile::Config, "a = 1\n[b]\nc = 1\n[b]\n")]
    #[case::appearance(TomlFile::Appearance, "[card]\n[card]\n")]
    #[case::theme_noir(TomlFile::Theme("noir".to_owned()), "[colors]\n[colors]\n")]
    fn a_parse_error_keeps_its_cause(#[case] file: TomlFile, #[case] source: &str) {
        let parsed: Result<toml::Table, Error> = parse_toml(source, file.clone());
        let error = parsed.expect_err("duplicate tables must not parse");

        assert!(std::error::Error::source(&error).is_some());
        let text = error.to_string();
        let place = text.lines().last().unwrap();
        assert!(
            place.starts_with(&format!("{file}:")),
            "whole text was {text:?}"
        );
    }

    #[rstest]
    #[case::config(TomlFile::Config, "config.toml")]
    #[case::appearance(TomlFile::Appearance, "sifr-ui.toml")]
    #[case::theme(TomlFile::Theme("noir".to_owned()), "noir.toml")]
    fn toml_file_names_the_file_on_disk(
        #[case] file: TomlFile,
        #[case] expected: &str,
    ) {
        assert_eq!(file.to_string(), expected);
    }
}
