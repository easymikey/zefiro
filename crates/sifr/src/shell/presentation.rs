use std::path::PathBuf;

use config::theme_file::{TomlColors, TomlTheme};
use kernel::domain::appearance::Appearance;
use widgets::{
    key_hints::KeyHintChords,
    scene::PixelPath,
    spectrum::Spectrum,
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
    pub(in crate::shell) home: Option<PathBuf>,
    pub(in crate::shell) spectrum: Spectrum,
    pub(in crate::shell) key_hint_chords: KeyHintChords,
}

pub(in crate::shell) fn theme(raw: TomlTheme) -> Theme {
    let TomlColors {
        background,
        muted_foreground,
        foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    } = raw.colors;
    let seed = ThemeBase {
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
        name: raw.name,
        colors: Colors::derive(&seed),
        scanning_label: raw.scanning_label,
    }
}

#[cfg(test)]
pub(in crate::shell) fn test_presentation() -> ShellPresentation {
    ShellPresentation {
        theme: theme(crate::startup::fallback_theme()),
        appearance: Appearance::default(),
        pixel_path: PixelPath::Halfblocks,
        color_depth: ColorDepth::TrueColor,
        cell_aspect: widgets::geometry::DEFAULT_CELL_ASPECT,
        home: None,
        spectrum: [0.0; widgets::spectrum::SPECTRUM_BANDS],
        key_hint_chords: KeyHintChords::default(),
    }
}

#[cfg(test)]
mod tests {
    use config::{embedded_theme::EMBEDDED_THEMES, theme_file::parse_theme};

    use crate::shell::presentation::theme;

    #[test]
    fn every_repo_theme_derives_its_own_palette() {
        for &(name, source) in EMBEDDED_THEMES {
            let colors = theme(parse_theme(source, name).unwrap()).colors;
            insta::with_settings!({ snapshot_suffix => name }, {
                insta::assert_debug_snapshot!(colors);
            });
        }
    }
}
