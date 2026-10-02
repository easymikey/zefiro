use kernel::domain::{Crossfade, OutputDevice, ReplayGain};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineConfig {
    pub crossfade: Crossfade,
    pub replay_gain: ReplayGain,
    pub device: OutputDevice,
}
