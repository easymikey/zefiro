use std::convert::Infallible;

use kernel::{
    cmd::ConfigPatch,
    domain::appearance::{Appearance, AppearancePatch},
    message::ConfigEvent,
    update::machine::LoopCmd,
};

use crate::{
    driver::{message::ConfigMessage, watch::ConfigWatchEffect},
    theme_file::TomlTheme,
};

pub(crate) type ConfigLoopCmd =
    LoopCmd<ConfigEffect, Infallible, ConfigMessage, ConfigEvent>;

#[derive(Debug, PartialEq)]
pub enum ConfigEffect {
    Watch(ConfigWatchEffect),
    SaveConfig(ConfigPatch),
    SaveAppearance(AppearancePatch),
    PublishTheme(TomlTheme),
    PublishAppearance(Appearance),
}
