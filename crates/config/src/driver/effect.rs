use std::{path::PathBuf, time::Duration};

use kernel::{
    cmd::ConfigPatch,
    domain::{appearance::AppearancePatch, config::ConfigName, revision::Revision},
};

use crate::{appearance_file::TomlAppearance, theme_file::TomlTheme};

#[derive(Debug, PartialEq)]
pub enum ConfigEffect {
    Watch(PathBuf),
    Read { file: ConfigName, path: PathBuf },
    List(PathBuf),
    After { delay: Duration, revision: Revision },
    SaveConfig(ConfigPatch),
    SaveAppearance(AppearancePatch),
    PublishTheme(TomlTheme),
    PublishAppearance(TomlAppearance),
}
