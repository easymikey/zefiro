#![forbid(unsafe_code)]
use kernel::{domain::percent::Percent, message::MacosEvent, update::machine::LoopCmd};
use objc2_core_audio::AudioObjectID;

use crate::{job::MacosJob, message::MacosMessage};

pub(crate) type MacosLoopCmd = LoopCmd<MacosEffect, MacosJob, MacosMessage, MacosEvent>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosEffect {
    Watch,
    Poll,
    Rebind(AudioObjectID),
    SetVolume(Percent),
    Publish,
    ClearArtwork,
    ShowArtwork(Vec<u8>),
}
