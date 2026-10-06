#![forbid(unsafe_code)]

use kernel::{
    cmd::{Cmds, MacosCmd},
    domain::revision::Revision,
    message::MacosError,
};

use crate::{hardware::HardwareMessage, remote_input::RemoteInput};

#[derive(Debug)]
pub enum MacosMessage {
    Started,
    Cmds(Cmds<MacosCmd>),
    HardwareChanged,
    Watched,
    Hardware(HardwareMessage),
    Error(MacosError),
    CoverRead(CoverBytes),
    Remote(RemoteInput),
}

impl From<Cmds<MacosCmd>> for MacosMessage {
    fn from(cmds: Cmds<MacosCmd>) -> Self {
        MacosMessage::Cmds(cmds)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverBytes {
    pub(crate) revision: Revision,
    pub(crate) bytes: Result<Vec<u8>, MacosError>,
}
