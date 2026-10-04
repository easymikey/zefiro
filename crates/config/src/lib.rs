#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod appearance;
mod appearance_file;
mod config_file;
mod driver;
mod embedded_theme;
mod error;
mod keymap;
mod patch;
pub(crate) mod theme_file;

pub use appearance_file::{
    APPEARANCE_FILE_NAME,
    TextCoverCells,
    TomlAppearance,
    TomlLayout,
    TomlProgress,
    TomlWindow,
    parse_appearance,
};
pub use config_file::{
    CONFIG_FILE_NAME,
    ConfigSettings,
    ConfigToml,
    parse_config,
    parse_config_reload,
};
pub use driver::{
    ConfigChange,
    ConfigDriver,
    ConfigEffect,
    ConfigMessage,
    ConfigPaths,
    SeenTexts,
};
pub use embedded_theme::{EMBEDDED_THEMES, embedded_theme, resolve_theme};
pub use error::Error;
pub use keymap::TomlKeymap;
pub use patch::{patch_appearance_text, patch_config_text};
pub use theme_file::{TomlColors, TomlTheme, parse_theme, theme_file_name};
