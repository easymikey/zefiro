#![forbid(unsafe_code)]

use kernel::{
    cmd::{Cmds, MacosCmd},
    domain::{percent::Percent, revision::Revision},
    message::MacosError,
};
use objc2_core_audio::AudioObjectID;

use crate::remote_input::RemoteInput;

#[derive(Debug)]
pub enum MacosMessage {
    Started,
    Cmds(Cmds<MacosCmd>),
    HardwareChanged,
    Watched,
    Polled(HardwarePoll),
    Rebound(AudioObjectID),
    VolumeSet(Percent),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwarePoll {
    pub(crate) tracked_device: AudioObjectID,
    pub(crate) current_device: AudioObjectID,
    pub(crate) volume: Option<Percent>,
}
