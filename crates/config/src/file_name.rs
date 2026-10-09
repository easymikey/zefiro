use std::path::{Path, PathBuf};

use kernel::domain::{config::ConfigName, theme::ThemeName};

pub const CONFIG_FILE_NAME: &str = "config.toml";

#[must_use]
pub fn theme_file_name(name: &str) -> String {
    format!("{name}.toml")
}

#[must_use]
pub(crate) fn theme_file_path(themes_dir: &Path, name: &ThemeName) -> PathBuf {
    themes_dir.join(theme_file_name(name.as_str()))
}

#[must_use]
pub(crate) fn config_file_name(name: &ConfigName) -> String {
    match name {
        ConfigName::Config => CONFIG_FILE_NAME.to_owned(),
        ConfigName::Theme(theme) => theme_file_name(theme.as_str()),
    }
}
