use kernel::domain::ThemeName;
use serde::{Deserialize, Deserializer};

use crate::{
    error::{Error, TomlFile, parse_toml},
    rgb::Rgb,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeColors {
    #[serde(rename = "bg")]
    pub background: Rgb,
    #[serde(rename = "fg")]
    pub foreground: Rgb,
    #[serde(rename = "bright_fg")]
    pub bright_foreground: Rgb,
    pub accent: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub red: Rgb,
    #[serde(default, rename = "window_bg")]
    pub window_background: Option<Rgb>,
}

const DEFAULT_SCANNING_LABEL: &str = "scanning…";

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
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    #[serde(deserialize_with = "theme_name")]
    pub name: ThemeName,
    pub colors: ThemeColors,
    #[serde(default = "default_scanning_label")]
    pub scanning_label: String,
}

#[must_use]
pub fn theme_file_name(name: &str) -> String {
    format!("{name}.toml")
}

pub fn parse_theme(source: &str, name: &str) -> Result<ThemeFile, Error> {
    parse_toml(source, TomlFile::Theme(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        embedded_theme::EMBEDDED_THEMES,
        error::Error,
        rgb::Rgb,
        theme_file::parse_theme,
    };

    #[test]
    fn every_repo_theme_parses() {
        for &(name, source) in EMBEDDED_THEMES {
            insta::with_settings!({ snapshot_suffix => name }, {
                insta::assert_debug_snapshot!(parse_theme(source, name).unwrap());
            });
        }
    }

    #[rstest]
    #[case::an_unknown_top_level_key(
        "unknown_top_level_key",
        "name = \"x\"\nbogus = 1\n[colors]\n\
         bg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\n\
         green = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n"
    )]
    #[case::an_unknown_color_key(
        "unknown_color_key",
        "name = \"x\"\n[colors]\n\
         bg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\n\
         green = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\nselection_bg = \"#ffd23f\"\n"
    )]
    #[case::an_invalid_hex_value(
        "invalid_hex_value",
        "name = \"x\"\n[colors]\n\
         bg = \"#zzzzzz\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\n\
         green = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n"
    )]
    fn a_strict_parse_rejects_what_it_does_not_recognise(
        #[case] name: &str,
        #[case] source: &str,
    ) {
        let error = parse_theme(source, "noir").unwrap_err();
        assert!(matches!(error, Error::Parse { .. }));
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(error.to_string());
        });
    }

    #[test]
    fn a_broken_theme_names_its_own_file_and_line() {
        let error =
            parse_theme("[colors]\nbg = \"#000000\"\n[colors]\n", "noir").unwrap_err();
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
    fn an_explicit_window_bg_key_is_carried_through_as_data() {
        let theme =
            parse_theme(include_str!("../../../themes/gruvbox.toml"), "gruvbox")
                .unwrap();

        assert_eq!(
            theme.colors.window_background,
            Some(Rgb([0x32, 0x30, 0x2f]))
        );
    }
}
