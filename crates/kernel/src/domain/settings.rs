use crate::domain::{
    appearance::AppearanceSettings,
    crossfade::Crossfade,
    device::{DeviceName, ListedDevice, OutputDevice},
    sleep_presets::SleepPresets,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioSettings {
    pub crossfade: Crossfade,
    pub replay_gain: ReplayGain,
    pub device: OutputDevice,
    pub sleep_presets: SleepPresets,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub audio_settings: AudioSettings,
    pub output_devices: Vec<ListedDevice>,
    pub device_name: Option<DeviceName>,
    pub appearance_settings: AppearanceSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayGain {
    On,
    #[default]
    Off,
}
