use std::{fmt, time::Duration};

use crate::domain::{
    Crossfade,
    SleepPresets,
    appearance::Appearance,
    time::SECONDS_PER_MINUTE,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceName(String);

impl DeviceName {
    pub fn new(name: String) -> Result<Self, DeviceNameError> {
        if name.is_empty() {
            return Err(DeviceNameError::Empty);
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeviceNameError {
    #[error("enter a device name")]
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDefault {
    Default,
    Named,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum OutputDevice {
    #[default]
    SystemDefault,
    Named(DeviceName),
}

impl OutputDevice {
    #[must_use]
    pub fn named(&self) -> Option<&DeviceName> {
        match self {
            Self::SystemDefault => None,
            Self::Named(name) => Some(name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedDevice {
    pub name: DeviceName,
    pub default: DeviceDefault,
}

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
    pub appearance: Appearance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayGain {
    On,
    #[default]
    Off,
}

#[must_use]
pub fn format_sleep_presets_label(presets: &[Duration]) -> String {
    if presets.is_empty() {
        return "off".to_string();
    }
    presets
        .iter()
        .map(|preset| format!("{}m", preset.as_secs() / SECONDS_PER_MINUTE))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::settings::{
        DeviceName,
        DeviceNameError,
        format_sleep_presets_label,
    };

    #[test]
    fn sleep_presets_label_renders_minutes_or_off() {
        assert_eq!(
            format_sleep_presets_label(&[
                Duration::from_secs(15 * 60),
                Duration::from_secs(30 * 60),
                Duration::from_secs(60 * 60),
            ]),
            "15m, 30m, 60m"
        );
        assert_eq!(format_sleep_presets_label(&[]), "off");
    }

    #[rstest]
    #[case::empty("".to_string(), Err(DeviceNameError::Empty))]
    #[case::named("Speakers".to_string(), Ok(()))]
    fn a_device_name_is_never_empty(
        #[case] name: String,
        #[case] expected: Result<(), DeviceNameError>,
    ) {
        assert_eq!(DeviceName::new(name).map(|_| ()), expected);
    }
}
