use audio::{
    AudioLoop,
    EngineConfig,
    SpectrumTap,
    UnityVolume,
    prepare as prepare_engine,
};
use crossbeam_channel::{Receiver, Sender};
use kernel::{AudioCmd, Message, domain::Startup};

use crate::driver::DriverLoop;

pub(crate) fn prepare(startup: &Startup) -> (AudioLoop, SpectrumTap) {
    prepare_engine(engine_config(startup))
}

impl DriverLoop<AudioCmd> for AudioLoop {
    fn run(self, inbox: &Receiver<AudioCmd>, mailbox: &Sender<Message>) {
        AudioLoop::run(self, inbox, mailbox);
    }
}

fn engine_config(startup: &Startup) -> EngineConfig {
    EngineConfig {
        crossfade: startup.crossfade,
        replaygain: startup.replaygain,
        unity_volume: unity_volume(),
        device: startup.output_device.clone(),
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
    use kernel::domain::Startup;

    use crate::audio::{engine_config, unity_volume};

    fn stock_startup() -> Startup {
        Startup {
            output_device: Some("Speakers".to_owned()),
            ..Startup::default()
        }
    }

    #[test]
    fn engine_config_carries_the_startup_device_and_gain_settings() {
        let startup = stock_startup();

        let config = engine_config(&startup);

        assert_eq!(config.crossfade, startup.crossfade);
        assert_eq!(config.replaygain, startup.replaygain);
        assert_eq!(config.device, startup.output_device);
        assert_eq!(config.unity_volume, unity_volume());
    }
}
