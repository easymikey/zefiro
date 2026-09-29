use kernel::domain::{ThemeChoice, ThemeName};

const TERRACOTTA_DARK: &str = include_str!("../../../themes/terracotta-dark.toml");
const TERRACOTTA_LIGHT: &str = include_str!("../../../themes/terracotta-light.toml");
const EMBER: &str = include_str!("../../../themes/ember.toml");
const GRUVBOX: &str = include_str!("../../../themes/gruvbox.toml");
const GRUVBOX_LIGHT: &str = include_str!("../../../themes/gruvbox-light.toml");
const HACKER: &str = include_str!("../../../themes/hacker.toml");
const MACAROON: &str = include_str!("../../../themes/macaroon.toml");
const NEOBRUTALISM_DARK: &str = include_str!("../../../themes/neobrutalism-dark.toml");
const NEOBRUTALISM_LIGHT: &str =
    include_str!("../../../themes/neobrutalism-light.toml");
const NOIR: &str = include_str!("../../../themes/noir.toml");
const OREO: &str = include_str!("../../../themes/oreo.toml");
const RISTRETTO: &str = include_str!("../../../themes/ristretto.toml");
const ROSE_PINE: &str = include_str!("../../../themes/rose-pine.toml");
const ROSE_PINE_DAWN: &str = include_str!("../../../themes/rose-pine-dawn.toml");
const WAFER: &str = include_str!("../../../themes/wafer.toml");
const WINAMP: &str = include_str!("../../../themes/winamp.toml");

pub const EMBEDDED_THEMES: &[&str] = &[
    "terracotta-dark",
    "terracotta-light",
    "ember",
    "gruvbox",
    "gruvbox-light",
    "hacker",
    "macaroon",
    "neobrutalism-dark",
    "neobrutalism-light",
    "noir",
    "oreo",
    "ristretto",
    "rose-pine",
    "rose-pine-dawn",
    "wafer",
    "winamp",
];

#[must_use]
pub fn embedded_theme(name: &str) -> Option<&'static str> {
    match name {
        "terracotta-dark" => Some(TERRACOTTA_DARK),
        "terracotta-light" => Some(TERRACOTTA_LIGHT),
        "ember" => Some(EMBER),
        "gruvbox" => Some(GRUVBOX),
        "gruvbox-light" => Some(GRUVBOX_LIGHT),
        "hacker" => Some(HACKER),
        "macaroon" => Some(MACAROON),
        "neobrutalism-dark" => Some(NEOBRUTALISM_DARK),
        "neobrutalism-light" => Some(NEOBRUTALISM_LIGHT),
        "noir" => Some(NOIR),
        "oreo" => Some(OREO),
        "ristretto" => Some(RISTRETTO),
        "rose-pine" => Some(ROSE_PINE),
        "rose-pine-dawn" => Some(ROSE_PINE_DAWN),
        "wafer" => Some(WAFER),
        "winamp" => Some(WINAMP),
        _ => None,
    }
}

#[must_use]
pub fn resolve_theme(choice: &ThemeChoice) -> ThemeName {
    match choice {
        ThemeChoice::Named(name) => name.clone(),
        ThemeChoice::Auto => ThemeName::from_static("noir"),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{ThemeChoice, ThemeName};
    use rstest::rstest;

    use crate::embedded_theme::{EMBEDDED_THEMES, embedded_theme, resolve_theme};

    #[rstest]
    #[case::auto(ThemeChoice::Auto, "noir")]
    #[case::named(ThemeChoice::Named(ThemeName::from_static("ember")), "ember")]
    fn a_choice_resolves_to_a_named_theme(
        #[case] choice: ThemeChoice,
        #[case] expected: &str,
    ) {
        assert_eq!(resolve_theme(&choice).as_str(), expected);
    }

    #[test]
    fn every_embedded_name_resolves_to_its_own_text() {
        for name in EMBEDDED_THEMES {
            assert!(embedded_theme(name).is_some());
        }
    }

    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        assert!(embedded_theme("not-a-theme").is_none());
    }
}
