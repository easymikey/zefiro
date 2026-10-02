use audio::{AudioLoop, EngineConfig, SpectrumTap};
use kernel::domain::Model;

pub(crate) fn audio_loop(model: &Model) -> (AudioLoop, SpectrumTap) {
    AudioLoop::new(engine_config(model))
}

fn engine_config(model: &Model) -> EngineConfig {
    EngineConfig {
        crossfade: model.settings.audio.crossfade,
        replay_gain: model.settings.audio.replay_gain,
        device: model.settings.audio.device.clone(),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{AudioSettings, DeviceName, Model, OutputDevice, Startup};

    use crate::audio::engine_config;

    fn stock_model() -> Model {
        let startup = Startup {
            audio: AudioSettings {
                device: OutputDevice::Named(
                    DeviceName::new("Speakers".to_string()).unwrap(),
                ),
                ..AudioSettings::default()
            },
            ..Startup::default()
        };
        let (model, _cmd) = kernel::startup(startup);
        model
    }

    #[test]
    fn engine_config_carries_the_startup_device_and_gain_settings() {
        let model = stock_model();

        let config = engine_config(&model);

        assert_eq!(config.crossfade, model.settings.audio.crossfade);
        assert_eq!(config.replay_gain, model.settings.audio.replay_gain);
        assert_eq!(config.device, model.settings.audio.device);
    }
}
