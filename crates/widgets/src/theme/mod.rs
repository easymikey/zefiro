use config::ThemeFile;

mod active_theme;
mod bars;
mod contrast;
mod palette;
mod rgb;
mod vinyl_colors;

pub use active_theme::ActiveTheme;
pub(crate) use bars::FillColors;
pub use bars::bar_colors;
pub use palette::{Colors, Role};
pub(crate) use rgb::shade;
pub use rgb::{ColorDepth, color_at_depth, lerp_rgb};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
    pub scanning_label: String,
}

impl From<ThemeFile> for Theme {
    fn from(file: ThemeFile) -> Self {
        Theme {
            name: file.name,
            colors: Colors::derive(&file.colors),
            scanning_label: file.scanning_label,
        }
    }
}
