use config::{AppearanceFile, AppearancePatch};
use runtime::Reload;
use widgets::Theme;

pub(crate) fn install(
    theme: &mut Theme,
    appearance: &mut AppearanceFile,
    reload: Reload,
) {
    match reload {
        Reload::Theme(file) => *theme = Theme::from(file),
        Reload::Appearance(file) => *appearance = file,
    }
}

pub(crate) fn apply_patch(appearance: &mut AppearanceFile, patch: AppearancePatch) {
    *appearance = appearance.patched(patch);
}

#[cfg(test)]
mod tests {
    use config::{
        Animations,
        AppearanceFile,
        AppearancePatch,
        Hex,
        ThemeColors,
        ThemeFile,
    };
    use rstest::rstest;
    use runtime::Reload;
    use widgets::Theme;

    use crate::shell::reload::{apply_patch, install};

    fn theme_file(name: &str) -> ThemeFile {
        ThemeFile {
            name: name.to_string(),
            colors: ThemeColors {
                background: Hex([0, 0, 0]),
                foreground: Hex([1, 1, 1]),
                bright_foreground: Hex([2, 2, 2]),
                accent: Hex([3, 3, 3]),
                green: Hex([0, 0xff, 0]),
                yellow: Hex([0xff, 0xff, 0]),
                red: Hex([0xff, 0, 0]),
                window_background: None,
            },
            scanning_label: "scanning…".to_string(),
        }
    }

    #[rstest]
    fn a_theme_reload_installs_the_parsed_theme() {
        let mut theme = Theme::from(theme_file("before"));
        let mut appearance = AppearanceFile::default();

        install(
            &mut theme,
            &mut appearance,
            Reload::Theme(theme_file("after")),
        );

        assert_eq!(theme.name, "after");
    }

    #[rstest]
    fn an_appearance_reload_replaces_the_appearance_file() {
        let mut theme = Theme::from(theme_file("noir"));
        let mut appearance = AppearanceFile::default();
        let mut replacement = AppearanceFile::default();
        replacement.cover.size_px = 512;

        install(&mut theme, &mut appearance, Reload::Appearance(replacement));

        assert_eq!(appearance.cover.size_px, 512);
    }

    #[rstest]
    fn a_patch_is_applied_on_top_of_the_current_appearance() {
        let mut appearance = AppearanceFile::default();
        let patch = AppearancePatch::builder()
            .animations(Animations::Off)
            .build();

        apply_patch(&mut appearance, patch);

        assert_eq!(appearance.window.animations, Animations::Off);
    }
}
