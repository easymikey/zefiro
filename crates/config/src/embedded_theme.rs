use kernel::domain::theme::{ThemeChoice, ThemeName};

pub const STOCK_THEME: &str = "noir";

pub const EMBEDDED_THEMES: &[(&str, &str)] = &[
    (
        "terracotta-dark",
        include_str!("../../../themes/terracotta-dark.toml"),
    ),
    (
        "terracotta-light",
        include_str!("../../../themes/terracotta-light.toml"),
    ),
    ("ember", include_str!("../../../themes/ember.toml")),
    ("gruvbox", include_str!("../../../themes/gruvbox.toml")),
    (
        "gruvbox-light",
        include_str!("../../../themes/gruvbox-light.toml"),
    ),
    ("hacker", include_str!("../../../themes/hacker.toml")),
    ("macaroon", include_str!("../../../themes/macaroon.toml")),
    (
        "neobrutalism-dark",
        include_str!("../../../themes/neobrutalism-dark.toml"),
    ),
    (
        "neobrutalism-light",
        include_str!("../../../themes/neobrutalism-light.toml"),
    ),
    ("noir", include_str!("../../../themes/noir.toml")),
    ("oreo", include_str!("../../../themes/oreo.toml")),
    ("ristretto", include_str!("../../../themes/ristretto.toml")),
    ("rose-pine", include_str!("../../../themes/rose-pine.toml")),
    (
        "rose-pine-dawn",
        include_str!("../../../themes/rose-pine-dawn.toml"),
    ),
    ("wafer", include_str!("../../../themes/wafer.toml")),
    ("winamp", include_str!("../../../themes/winamp.toml")),
];

#[must_use]
pub fn embedded_theme(name: &str) -> Option<&'static str> {
    EMBEDDED_THEMES
        .iter()
        .find_map(|&(embedded, text)| (embedded == name).then_some(text))
}

#[must_use]
pub fn resolve_theme(choice: &ThemeChoice) -> ThemeName {
    match choice {
        ThemeChoice::Named(name) => name.clone(),
        ThemeChoice::Auto => ThemeName::from_static(STOCK_THEME),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::theme::{ThemeChoice, ThemeName};
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
        for &(name, text) in EMBEDDED_THEMES {
            assert_eq!(embedded_theme(name), Some(text));
        }
    }

    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        assert!(embedded_theme("not-a-theme").is_none());
    }
}
