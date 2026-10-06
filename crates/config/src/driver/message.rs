use kernel::{
    cmd::{Cmds, ConfigCmd},
    domain::{
        config::{ConfigError, ConfigName},
        io_error::IoError,
        revision::Revision,
    },
};
use strum::IntoStaticStr;

use crate::driver::watch::ConfigWatchMessage;

#[derive(Debug, PartialEq, IntoStaticStr)]
pub enum ConfigMessage {
    Cmds(Cmds<ConfigCmd>),
    Started,
    Changed(Result<(), IoError>),
    Watch(ConfigWatchMessage),
    Elapsed(Revision),
    Saved { name: ConfigName, text: String },
    Error(ConfigError),
}

impl From<Cmds<ConfigCmd>> for ConfigMessage {
    fn from(cmds: Cmds<ConfigCmd>) -> Self {
        ConfigMessage::Cmds(cmds)
    }
}
