use kernel::{
    cmd::{Cmds, ConfigCmd},
    domain::{
        config::{ConfigError, ConfigName},
        io_error::IoError,
        revision::Revision,
        theme::ThemeName,
    },
};
use strum::IntoStaticStr;

#[derive(Debug, PartialEq, IntoStaticStr)]
pub enum ConfigMessage {
    Cmds(Cmds<ConfigCmd>),
    Started,
    Changed(Result<(), IoError>),
    ReadDone {
        file: ConfigName,
        text: Option<String>,
    },
    Listed(Vec<ThemeName>),
    Elapsed(Revision),
    Saved {
        file: ConfigName,
        text: String,
    },
    Reloaded(ConfigChange),
    Error(ConfigError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChange {
    Appearance(Option<String>),
    Keymap(Option<String>),
    Theme {
        name: ThemeName,
        text: Option<String>,
    },
    Themes(Vec<ThemeName>),
}

impl From<Cmds<ConfigCmd>> for ConfigMessage {
    fn from(cmds: Cmds<ConfigCmd>) -> Self {
        ConfigMessage::Cmds(cmds)
    }
}
