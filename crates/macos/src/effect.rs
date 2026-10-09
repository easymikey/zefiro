#![forbid(unsafe_code)]
use kernel::domain::percent::Percent;
use objc2_core_audio::AudioObjectID;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosEffect {
    Listen,
    Poll,
    Rebind(AudioObjectID),
    SetVolume(Percent),
    ShowNowPlaying,
    ClearArtwork,
    ShowArtwork(Vec<u8>),
    Privacy,
}
