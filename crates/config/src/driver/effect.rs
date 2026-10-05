use std::convert::Infallible;

use kernel::{
    cmd::ConfigPatch,
    domain::appearance::{Appearance, AppearancePatch},
    message::ConfigEvent,
    update::machine::LoopCmd,
};

use crate::{
    driver::{message::ConfigMessage, watch::WatchEffect},
    theme_file::TomlTheme,
};

pub(crate) type ConfigLoopCmd =
    LoopCmd<ConfigEffect, Infallible, ConfigMessage, ConfigEvent>;

#[derive(Debug, PartialEq)]
pub enum ConfigEffect {
    Watch(WatchEffect),
    SaveConfig(ConfigPatch),
    SaveAppearance(AppearancePatch),
    PublishTheme(TomlTheme),
    PublishAppearance(Appearance),
}
