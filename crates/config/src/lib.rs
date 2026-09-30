#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod appearance;
pub(crate) mod appearance_document;
mod appearance_file;
mod breakpoints;
pub(crate) mod config_document;
mod config_file;
pub(crate) mod document;
mod embedded_theme;
mod error;
pub(crate) mod hex;
mod keymap;
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
    preset_appearance,
    preset_of,
};
pub use appearance_document::patch_appearance_text;
pub use appearance_file::{
    APPEARANCE_FILE_NAME,
    AppearanceFile,
    CardConfig,
    CoverConfig,
    ProgressConfig,
    TextCoverCells,
    WindowConfig,
    parse_appearance,
};
pub use breakpoints::LayoutConfig;
pub use config_document::patch_config_text;
pub use config_file::{AudioConfig, CONFIG_FILE_NAME, ConfigToml, parse_config};
pub use embedded_theme::{EMBEDDED_THEMES, embedded_theme, resolve_theme};
pub use error::{ColorError, CrossfadeError, Error, SettingError, TomlFile};
pub use hex::Rgb;
pub use keymap::{ConfigReload, KeymapFile, parse_config_reload};
pub use rows::{
    APPEARANCE_ROWS,
    AppearanceField,
    AppearanceRow,
    appearance_patch,
    appearance_row,
    custom_settings,
};
pub use theme_file::{ThemeColors, ThemeFile, parse_theme, theme_file_name};
