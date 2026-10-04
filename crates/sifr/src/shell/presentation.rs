use std::path::PathBuf;

use config::{
    appearance_file::TomlAppearance,
    theme_file::{TomlColors, TomlTheme},
};
use kernel::domain::geometry::Cells;
use widgets::{
    appearance::Appearance,
    geometry::CoverCells,
    primitive::bar::ProgressBar,
    scene::PixelPath,
    screen::breakpoint::Breakpoints,
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
}

pub(in crate::shell) fn theme(raw: TomlTheme) -> Theme {
    let TomlColors {
        background,
        foreground,
        bright_foreground,
        accent,
        green,
        yellow,
        red,
        window_background,
    } = raw.colors;
    let seed = ThemeBase {
        background,
        foreground,
        bright_foreground,
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

pub(crate) fn appearance(raw: &TomlAppearance) -> Appearance {
    Appearance {
        cover_cells: CoverCells {
            width: Cells(raw.cover.text_cells.width),
            height: Cells(raw.cover.text_cells.height),
        },
        breakpoints: Breakpoints {
            full_min_width: Cells(raw.layout.full_min_width),
            full_min_height: Cells(raw.layout.full_min_height),
            compact_min_width: Cells(raw.layout.compact_min_width),
            compact_min_height: Cells(raw.layout.compact_min_height),
            min_columns: Cells(raw.layout.min_columns),
            min_rows: Cells(raw.layout.min_rows),
        },
        progress: ProgressBar {
            height: raw.progress.height_px,
            radius: raw.progress.radius,
            fill: raw.progress.fill,
            groove: raw.progress.groove,
        },
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
    }
}
