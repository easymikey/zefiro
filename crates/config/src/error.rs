use std::num::ParseIntError;

use kernel::domain::{ConfigName, ThemeNameError};
use serde::de::DeserializeOwned;

use crate::{
    appearance_file::APPEARANCE_FILE_NAME,
    config_file::CONFIG_FILE_NAME,
    theme_file::theme_file_name,
};

#[must_use]
pub(crate) fn config_file_name(name: &ConfigName) -> String {
    match name {
        ConfigName::Config => CONFIG_FILE_NAME.to_owned(),
        ConfigName::Appearance => APPEARANCE_FILE_NAME.to_owned(),
        ConfigName::Theme(theme) => theme_file_name(theme.as_str()),
    }
}

fn first_line(error: &toml::de::Error) -> &str {
    let message = error.message();
    message.lines().next().unwrap_or(message).trim()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{}\n{}:{line}", first_line(.source), config_file_name(.file))]
    Parse {
        file: ConfigName,
        line: usize,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error(transparent)]
    ThemeName(#[from] ThemeNameError),
    #[error("`{key}` is not a table")]
    NotATable { key: String },
    #[error("{}", .0.message())]
    Document(#[from] toml_edit::TomlError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CrossfadeError {
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
    OutOfRange(#[from] kernel::domain::CrossfadeError),
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

pub(crate) fn parse_toml<T>(source: &str, file: ConfigName) -> Result<T, Error>
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
    use kernel::domain::{ConfigName, ThemeName};
    use rstest::rstest;

    use crate::error::{Error, config_file_name, parse_toml};

    fn error_text(source: &str, file: ConfigName) -> String {
        let parsed: Result<toml::Table, Error> = parse_toml(source, file);
        parsed
            .err()
            .map_or_else(String::new, |error| error.to_string())
    }

    #[test]
    fn an_error_puts_the_message_first_and_the_place_below_it() {
        let text =
            error_text("a = 1\n\n[card]\nb = 2\n[card]\n", ConfigName::Appearance);

        let place = text.lines().nth(1);

        assert_eq!(place, Some("sifr-ui.toml:5"), "whole text was {text:?}");
    }

    #[test]
    fn an_error_on_the_first_line_reports_line_one_in_exactly_two_lines() {
        let text = error_text("[card\n", ConfigName::Config);

        assert_eq!(text.lines().nth(1), Some("config.toml:1"));
        assert_eq!(text.lines().count(), 2, "whole text was {text:?}");
    }

    #[rstest]
    #[case::config(ConfigName::Config, "a = 1\n[b]\nc = 1\n[b]\n")]
    #[case::appearance(ConfigName::Appearance, "[card]\n[card]\n")]
    #[case::theme_noir(
        ConfigName::Theme(ThemeName::from_static("noir")),
        "[colors]\n[colors]\n"
    )]
    fn a_parse_error_keeps_its_cause(#[case] file: ConfigName, #[case] source: &str) {
        let parsed: Result<toml::Table, Error> = parse_toml(source, file.clone());
        let error = parsed.expect_err("duplicate tables must not parse");

        assert!(std::error::Error::source(&error).is_some());
        let text = error.to_string();
        let place = text.lines().last().unwrap();
        assert!(
            place.starts_with(&format!("{}:", config_file_name(&file))),
            "whole text was {text:?}"
        );
    }

    #[rstest]
    #[case::config(ConfigName::Config, "config.toml")]
    #[case::appearance(ConfigName::Appearance, "sifr-ui.toml")]
    #[case::theme(ConfigName::Theme(ThemeName::from_static("noir")), "noir.toml")]
    fn toml_file_names_the_file_on_disk(
        #[case] file: ConfigName,
        #[case] expected: &str,
    ) {
        assert_eq!(config_file_name(&file), expected);
    }
}
