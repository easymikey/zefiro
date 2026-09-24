use serde::Deserialize;

use crate::{
    error::{ConfigError, named_toml},
    hex::Hex,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeColors {
    #[serde(rename = "bg")]
    pub background: Hex,
    #[serde(rename = "fg")]
    pub foreground: Hex,
    #[serde(rename = "bright_fg")]
    pub bright_foreground: Hex,
    pub accent: Hex,
    pub green: Hex,
    pub yellow: Hex,
    pub red: Hex,
    #[serde(default, rename = "window_bg")]
    pub window_background: Option<Hex>,
}

const DEFAULT_SCANNING_LABEL: &str = "scanning…";

fn default_scanning_label() -> String {
    DEFAULT_SCANNING_LABEL.to_owned()
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub name: String,
    pub colors: ThemeColors,
    #[serde(default = "default_scanning_label")]
    pub scanning_label: String,
}

#[must_use]
pub fn theme_file_name(name: &str) -> String {
    format!("{name}.toml")
}

pub fn parse_theme(source: &str, name: &str) -> Result<ThemeFile, ConfigError> {
    named_toml(source, &theme_file_name(name))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{error::ConfigError, hex::Hex, theme_file::parse_theme};

    #[rstest]
    #[case::terracotta_dark(
        "terracotta-dark",
        include_str!("../../../themes/terracotta-dark.toml")
    )]
    #[case::terracotta_light(
        "terracotta-light",
        include_str!("../../../themes/terracotta-light.toml")
    )]
    #[case::ember("ember", include_str!("../../../themes/ember.toml"))]
    #[case::gruvbox("gruvbox", include_str!("../../../themes/gruvbox.toml"))]
    #[case::gruvbox_light(
        "gruvbox-light",
        include_str!("../../../themes/gruvbox-light.toml")
    )]
    #[case::hacker("hacker", include_str!("../../../themes/hacker.toml"))]
    #[case::macaroon("macaroon", include_str!("../../../themes/macaroon.toml"))]
    #[case::neobrutalism_dark(
        "neobrutalism-dark",
        include_str!("../../../themes/neobrutalism-dark.toml")
    )]
    #[case::neobrutalism_light(
        "neobrutalism-light",
        include_str!("../../../themes/neobrutalism-light.toml")
    )]
    #[case::noir("noir", include_str!("../../../themes/noir.toml"))]
    #[case::oreo("oreo", include_str!("../../../themes/oreo.toml"))]
    #[case::ristretto("ristretto", include_str!("../../../themes/ristretto.toml"))]
    #[case::rose_pine("rose-pine", include_str!("../../../themes/rose-pine.toml"))]
    #[case::rose_pine_dawn(
        "rose-pine-dawn",
        include_str!("../../../themes/rose-pine-dawn.toml")
    )]
    #[case::wafer("wafer", include_str!("../../../themes/wafer.toml"))]
    #[case::winamp("winamp", include_str!("../../../themes/winamp.toml"))]
    fn every_repo_theme_parses(#[case] name: &str, #[case] source: &str) {
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_debug_snapshot!(parse_theme(source, name).unwrap());
        });
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
        assert!(matches!(error, ConfigError::Parse { .. }));
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
            Some(Hex([0x32, 0x30, 0x2f]))
        );
    }
}
