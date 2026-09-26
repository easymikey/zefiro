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
    ProgressStyle,
    SpeedChipMode,
    preset_of,
    preset_options,
};
pub use appearance_document::appearance_patched;
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
pub use breakpoints::BreakpointsConfig;
pub use config_document::patched;
pub use config_file::{AudioConfig, CONFIG_FILE_NAME, ConfigFile, parse_config};
pub use embedded_theme::{EMBEDDED_THEMES, embedded_theme};
pub use error::{
    ColorRejection,
    ConfigError,
    CrossfadeRejection,
    SettingRejection,
    TomlFile,
};
pub use hex::Hex;
pub use keymap::{KeymapFile, ParsedKeymap, parse_keymap};
pub use rows::{
    APPEARANCE_ROWS,
    AppearanceField,
    AppearanceRow,
    appearance_patch,
    appearance_row,
    custom_rows,
};
pub use theme_file::{ThemeColors, ThemeFile, parse_theme, theme_file_name};
