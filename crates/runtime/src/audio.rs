use audio::{AudioLoop, EngineConfig, SpectrumTap, UnityVolume};
use crossbeam_channel::Receiver;
use kernel::{AudioCmd, AudioEvent, domain::Model};

use crate::{driver::DriverLoop, sender::DriverSender};

pub(crate) fn prepare(model: &Model) -> (AudioLoop, SpectrumTap) {
    AudioLoop::new(engine_config(model))
}

impl DriverLoop<AudioCmd, AudioEvent> for AudioLoop {
    fn run(self, inbox: &Receiver<AudioCmd>, outbox: &DriverSender<AudioEvent>) {
        AudioLoop::run(self, inbox, outbox);
    }
}

fn engine_config(model: &Model) -> EngineConfig {
    EngineConfig {
        crossfade: model.settings.crossfade,
        replaygain: model.settings.replaygain,
        unity_volume: unity_volume(),
        device: model.settings.output_device.clone(),
    }
}

fn unity_volume() -> UnityVolume {
    if cfg!(target_os = "macos") {
        UnityVolume::Pinned
    } else {
        UnityVolume::Free
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{DeviceName, Model, OutputDevice, Startup};

    use crate::audio::{engine_config, unity_volume};

    fn stock_model() -> Model {
        let startup = Startup {
            output_device: OutputDevice::Named(
                DeviceName::new("Speakers".to_string()).unwrap(),
            ),
            ..Startup::default()
        };
        let (model, _cmd) = kernel::startup(startup);
        model
    }

    #[test]
    fn engine_config_carries_the_startup_device_and_gain_settings() {
        let model = stock_model();

        let config = engine_config(&model);

        assert_eq!(config.crossfade, model.settings.crossfade);
        assert_eq!(config.replaygain, model.settings.replaygain);
        assert_eq!(config.device, model.settings.output_device);
        assert_eq!(config.unity_volume, unity_volume());
    }
}
