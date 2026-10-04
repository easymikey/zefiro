use crate::domain::{
    appearance::AppearanceSettings,
    crossfade::Crossfade,
    device::{ListedDevice, OutputDevice},
    sleep_presets::SleepPresets,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioSettings {
    pub crossfade: Crossfade,
    pub replay_gain: ReplayGain,
    pub device: OutputDevice,
    pub sleep_presets: SleepPresets,
}

#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub audio: AudioSettings,
    pub output_devices: Vec<ListedDevice>,
    pub appearance: AppearanceSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayGain {
    On,
    #[default]
    Off,
}
