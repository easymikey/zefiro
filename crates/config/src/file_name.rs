use std::path::{Path, PathBuf};

use kernel::domain::{config::ConfigName, theme::ThemeName};

pub const CONFIG_FILE_NAME: &str = "config.toml";

pub const APPEARANCE_FILE_NAME: &str = "sifr-ui.toml";

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
        ConfigName::Appearance => APPEARANCE_FILE_NAME.to_owned(),
        ConfigName::Theme(theme) => theme_file_name(theme.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{config::ConfigName, theme::ThemeName};
    use rstest::rstest;

    use crate::file_name::config_file_name;

    #[rstest]
    #[case::config(ConfigName::Config, "config.toml")]
    #[case::appearance(ConfigName::Appearance, "sifr-ui.toml")]
    #[case::theme(ConfigName::Theme(ThemeName::from_static("noir")), "noir.toml")]
    fn toml_file_names_the_file_on_disk(
        #[case] config_name: ConfigName,
        #[case] expected: &str,
    ) {
        assert_eq!(config_file_name(&config_name), expected);
    }
}
