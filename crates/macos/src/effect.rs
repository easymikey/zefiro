#![forbid(unsafe_code)]

use kernel::domain::percent::Percent;
use objc2_core_audio::AudioObjectID;

use crate::job::MacosJob;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosEffect {
    Watch,
    Poll,
    Rebind(AudioObjectID),
    SetVolume(Percent),
    Publish,
    ClearArtwork,
    ShowArtwork(Vec<u8>),
    Run(MacosJob),
}
