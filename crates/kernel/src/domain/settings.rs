use std::time::Duration;

use crate::domain::{Crossfade, SLEEP_PRESET_BUNDLES, time::SECONDS_PER_MINUTE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDefault {
    Default,
    Named,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    pub name: String,
    pub default: DeviceDefault,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub crossfade: Crossfade,
    pub replaygain: Replaygain,
    pub output_device: Option<String>,
    pub output_devices: Vec<OutputDevice>,
    pub sleep_presets: Box<[Duration]>,
    pub theme: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Replaygain {
    On,
    #[default]
    Off,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            crossfade: Crossfade::default(),
            replaygain: Replaygain::Off,
            output_device: None,
            output_devices: Vec::new(),
            sleep_presets: SLEEP_PRESET_BUNDLES.first(),
            theme: "auto".to_string(),
        }
    }
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

    use crate::domain::settings::format_sleep_presets_label;

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
}
