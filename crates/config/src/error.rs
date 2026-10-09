use std::num::ParseIntError;

use kernel::domain::{
    config::ConfigName,
    theme::{ThemeName, ThemeNameError},
};
use serde::de::DeserializeOwned;

use crate::file_name::config_file_name;

fn first_line(error: &toml::de::Error) -> &str {
    let message = error.message();
    message.lines().next().unwrap_or(message).trim()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{}\n{}:{line}", first_line(.source), config_file_name(.name))]
    Parse {
        name: ConfigName,
        line: usize,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error(transparent)]
    ThemeName(#[from] ThemeNameError),
    #[error("`{0}` is not a table")]
    NotATable(&'static str),
    #[error("{}", .0.message())]
    Document(#[from] toml_edit::TomlError),
    #[error("no theme named `{0}`")]
    UnknownTheme(ThemeName),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CrossfadeTextError {
    #[error("invalid crossfade: {0}")]
    NotANumber(#[source] ParseIntError),
    #[error("invalid crossfade: expected an integer with an 's' or 'ms' suffix")]
    MissingSuffix,
    #[error(transparent)]
    OutOfRange(#[from] kernel::domain::crossfade::CrossfadeError),
}

pub(crate) fn line_at(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .unwrap_or(text)
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

pub(crate) fn parse_toml<T>(text: &str, config_name: ConfigName) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    toml::from_str(text).map_err(|error| Error::Parse {
        name: config_name,
        line: line_at(text, error.span().map_or(0, |span| span.start)),
        source: Box::new(error),
    })
}

#[cfg(test)]
mod tests {
    use kernel::domain::{config::ConfigName, theme::ThemeName};
    use rstest::rstest;

    use crate::{
        error::{Error, parse_toml},
        file_name::config_file_name,
    };

    #[rstest]
    #[case::config(ConfigName::Config, "a = 1\n[b]\nc = 1\n[b]\n")]
    #[case::theme_noir(
        ConfigName::Theme(ThemeName::from_static("noir")),
        "[colors]\n[colors]\n"
    )]
    fn a_parse_error_keeps_its_cause(
        #[case] config_name: ConfigName,
        #[case] text: &str,
    ) {
        let parsed: Result<toml::Table, Error> = parse_toml(text, config_name.clone());
        let error = parsed.expect_err("duplicate tables must not parse");

        assert!(std::error::Error::source(&error).is_some());
        let error_text = error.to_string();
        let place = error_text.lines().last().unwrap();
        assert!(
            place.starts_with(&format!("{}:", config_file_name(&config_name))),
            "whole text was {error_text:?}"
        );
    }
}
