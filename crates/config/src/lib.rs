#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

pub mod appearance;
pub(crate) mod appearance_document;
pub mod appearance_file;
pub mod breakpoints;
pub(crate) mod config_document;
pub mod config_file;
pub(crate) mod document;
pub mod embedded_theme;
pub mod error;
pub(crate) mod hex;
pub mod keymap;
pub mod rows;
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
pub use appearance_file::{AppearanceFile, parse_appearance};
pub use config_document::patched;
pub use config_file::{ConfigFile, parse};
pub use embedded_theme::{EMBEDDED_THEMES, embedded_theme};
pub use error::{ColorRejection, ConfigError, CrossfadeRejection, SettingRejection};
pub use hex::Hex;
pub use keymap::{ParsedKeymap, parse_keymap};
pub use rows::{
    APPEARANCE_ROWS,
    AppearanceField,
    AppearanceRow,
    appearance_patch,
    appearance_row,
    custom_rows,
};
pub use theme_file::{ThemeColors, ThemeFile, parse_theme, theme_file_name};
