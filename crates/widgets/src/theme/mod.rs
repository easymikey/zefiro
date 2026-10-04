use kernel::domain::ThemeName;

mod active_theme;
mod backdrop_style;
mod colors;
mod contrast;
mod rgb;

pub use active_theme::ActiveTheme;
pub(crate) use active_theme::{ProgressStyle, VolumeStyle};
pub use backdrop_style::BackdropStyle;
pub use colors::{Colors, Role, ThemeBase};
pub use rgb::{ColorDepth, color_at_depth, lerp_rgb, shade};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: ThemeName,
    pub colors: Colors,
    pub scanning_label: String,
}
