use kernel::domain::{appearance::Rgb, config::ConfigName, theme::ThemeName};
use serde::{Deserialize, Deserializer};

use crate::{
    appearance::{from_str_field, from_str_option},
    error::{Error, parse_toml},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a [colors] table of hex colours")]
pub struct TomlColors {
    #[serde(deserialize_with = "from_str_field")]
    pub background: Rgb,
    #[serde(deserialize_with = "from_str_field")]
    pub muted_foreground: Rgb,
    #[serde(deserialize_with = "from_str_field")]
    pub foreground: Rgb,
    #[serde(deserialize_with = "from_str_field")]
    pub accent: Rgb,
    #[serde(deserialize_with = "from_str_field")]
    pub green: Rgb,
    #[serde(deserialize_with = "from_str_field")]
    pub yellow: Rgb,
    #[serde(deserialize_with = "from_str_field")]
    pub red: Rgb,
    #[serde(default, deserialize_with = "from_str_option")]
    pub window_background: Option<Rgb>,
}

pub const DEFAULT_SCANNING_LABEL: &str = "scanning…";

fn theme_name<'de, D>(deserializer: D) -> Result<ThemeName, D::Error>
where
    D: Deserializer<'de>,
{
    ThemeName::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

fn default_scanning_label() -> String {
    DEFAULT_SCANNING_LABEL.to_owned()
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    deny_unknown_fields,
    expecting = "a theme file with a name and a [colors] table"
)]
pub struct TomlTheme {
    #[serde(deserialize_with = "theme_name")]
    pub name: ThemeName,
    pub colors: TomlColors,
    #[serde(default = "default_scanning_label")]
    pub scanning_label: String,
}

pub fn parse_theme(text: &str, name: &str) -> Result<TomlTheme, Error> {
    let theme = ThemeName::new(name.to_owned())?;
    parse_toml(text, ConfigName::Theme(theme))
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::Rgb;
    use rstest::rstest;

    use crate::{
        embedded_theme::EMBEDDED_THEMES,
        error::Error,
        theme_file::parse_theme,
    };

    #[test]
    fn every_repo_theme_parses() {
        for &(name, text) in EMBEDDED_THEMES {
            insta::with_settings!({ snapshot_suffix => name }, {
                insta::assert_debug_snapshot!(parse_theme(text, name).unwrap());
            });
        }
    }

    #[rstest]
    #[case::an_unknown_top_level_key(
        "unknown_top_level_key",
        "name = \"x\"\nbogus = 1\n[colors]\n\
         background = \"#000000\"\nmuted_foreground = \"#000000\"\nforeground = \"#000000\"\naccent = \"#000000\"\n\
         green = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n"
    )]
    #[case::an_unknown_color_key(
        "unknown_color_key",
        "name = \"x\"\n[colors]\n\
         background = \"#000000\"\nmuted_foreground = \"#000000\"\nforeground = \"#000000\"\naccent = \"#000000\"\n\
         green = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\nselection_background = \"#ffd23f\"\n"
    )]
    #[case::an_invalid_hex_value(
        "invalid_hex_value",
        "name = \"x\"\n[colors]\n\
         background = \"#zzzzzz\"\nmuted_foreground = \"#000000\"\nforeground = \"#000000\"\naccent = \"#000000\"\n\
         green = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n"
    )]
    fn a_strict_parse_rejects_what_it_does_not_recognise(
        #[case] name: &str,
        #[case] text: &str,
    ) {
        let error = parse_theme(text, "noir").unwrap_err();
        assert!(matches!(error, Error::Parse { .. }));
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(error.to_string());
        });
    }

    #[test]
    fn a_broken_theme_names_its_own_file_and_line() {
        let error =
            parse_theme("[colors]\nbackground = \"#000000\"\n[colors]\n", "noir")
                .unwrap_err();
        let text = error.to_string();

        assert_eq!(text.lines().nth(1), Some("noir.toml:3"));
    }

    #[test]
    fn an_absent_scanning_label_defaults_to_the_stock_wording() {
        let theme =
            parse_theme(include_str!("../../../themes/noir.toml"), "noir").unwrap();

        assert_eq!(theme.scanning_label, "scanning…");
    }

    #[test]
    fn an_explicit_window_background_key_is_carried_through_as_data() {
        let theme =
            parse_theme(include_str!("../../../themes/gruvbox.toml"), "gruvbox")
                .unwrap();

        assert_eq!(
            theme.colors.window_background,
            Some(Rgb([0x32, 0x30, 0x2f]))
        );
    }
}
