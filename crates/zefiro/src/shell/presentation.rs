use std::path::PathBuf;

use config::theme_file::{TomlColors, TomlTheme};
use kernel::domain::appearance::Appearance;
use widgets::{
    key_hints::KeyHintChords,
    scene::PixelPath,
    spectrum::{SPECTRUM_BANDS, Spectrum},
    theme::{
        Theme,
        colors::{Colors, ThemeBase},
        rgb::ColorDepth,
    },
};

pub(crate) struct ShellPresentation {
    pub(in crate::shell) theme: Theme,
    pub(in crate::shell) appearance: Appearance,
    pub(in crate::shell) pixel_path: PixelPath,
    pub(in crate::shell) color_depth: ColorDepth,
    pub(in crate::shell) cell_aspect: f32,
    pub(in crate::shell) home_dir: Option<PathBuf>,
    pub(in crate::shell) spectrum: Spectrum,
    pub(in crate::shell) key_hint_chords: KeyHintChords,
}

impl ShellPresentation {
    pub(in crate::shell) fn new(
        theme: Theme,
        pixel_path: PixelPath,
        color_depth: ColorDepth,
    ) -> Self {
        Self {
            theme,
            appearance: Appearance::default(),
            pixel_path,
            color_depth,
            cell_aspect: widgets::geometry::DEFAULT_CELL_ASPECT,
            home_dir: dirs::home_dir(),
            spectrum: [0.0; SPECTRUM_BANDS],
            key_hint_chords: KeyHintChords::default(),
        }
    }
}

pub(in crate::shell) fn theme(toml_theme: TomlTheme) -> Theme {
    let TomlColors {
        background,
        muted_foreground,
        foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    } = toml_theme.colors;
    let theme_base = ThemeBase {
        background,
        muted_foreground,
        foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    };
    Theme {
        name: toml_theme.name,
        colors: Colors::from_theme_base(&theme_base),
        scanning_label: toml_theme.scanning_label,
    }
}

#[cfg(test)]
mod tests {
    use config::{embedded_theme::EMBEDDED_THEMES, theme_file::parse_theme};

    use crate::shell::presentation::theme;

    #[test]
    fn every_repo_theme_derives_its_own_palette() {
        for &(name, text) in EMBEDDED_THEMES {
            let colors = theme(parse_theme(text, name).unwrap()).colors;
            insta::with_settings!({ snapshot_suffix => name }, {
                insta::assert_debug_snapshot!(colors);
            });
        }
    }
}
