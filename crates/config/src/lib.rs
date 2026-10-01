#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod appearance;
mod appearance_file;
mod config_file;
mod embedded_theme;
mod error;
mod keymap;
mod patch;
pub(crate) mod rgb;
mod rows;
pub(crate) mod theme_file;

pub use appearance::{
    Animations,
    Appearance,
    AppearancePatch,
    AppearancePreset,
    CoverBrackets,
    CoverStyle,
    FormatChips,
    KeyHints,
    LayoutMode,
    ProgressTime,
    SpeedChip,
    preset_of,
};
pub use appearance_file::{
    APPEARANCE_FILE_NAME,
    AppearanceFile,
    LayoutConfig,
    ProgressConfig,
    TextCoverCells,
    WindowConfig,
    parse_appearance,
};
pub use config_file::{
    CONFIG_FILE_NAME,
    ConfigReload,
    ConfigToml,
    parse_config,
    parse_config_reload,
};
pub use embedded_theme::{EMBEDDED_THEMES, embedded_theme, resolve_theme};
pub use error::Error;
pub use keymap::KeymapFile;
pub use patch::{patch_appearance_text, patch_config_text};
pub use rgb::Rgb;
pub use rows::{
    APPEARANCE_ROWS,
    AppearanceField,
    appearance_patch,
    appearance_row,
    custom_settings,
};
pub use theme_file::{ThemeColors, ThemeFile, parse_theme, theme_file_name};
