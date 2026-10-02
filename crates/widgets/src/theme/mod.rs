use kernel::domain::ThemeName;

mod active_theme;
mod colors;
mod contrast;
mod rgb;

pub use active_theme::ActiveTheme;
pub(crate) use active_theme::BarStyle;
pub use colors::{Colors, Role, ThemeSeed};
pub use rgb::{ColorDepth, color_at_depth, lerp_rgb, shade};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: ThemeName,
    pub colors: Colors,
    pub scanning_label: String,
}
