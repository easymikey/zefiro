#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![deny(unreachable_pub)]

mod appearance;
pub mod appearance_file;
pub mod config_file;
pub mod driver;
pub mod embedded_theme;
pub mod error;
pub(crate) mod keymap;
pub mod patch;
pub mod theme_file;
