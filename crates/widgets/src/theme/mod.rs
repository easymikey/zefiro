use kernel::domain::theme::ThemeName;

pub mod active_theme;
pub mod backdrop_style;
pub mod colors;
mod contrast;
pub mod rgb;

use colors::Colors;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: ThemeName,
    pub colors: Colors,
    pub scanning_label: String,
}
