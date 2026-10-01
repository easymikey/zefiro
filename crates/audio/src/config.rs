use kernel::domain::{Crossfade, OutputDevice, Replaygain};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineConfig {
    pub crossfade: Crossfade,
    pub replaygain: Replaygain,
    pub device: OutputDevice,
}
