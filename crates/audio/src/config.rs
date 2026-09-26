use kernel::domain::{Crossfade, DeviceName, Replaygain};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineConfig {
    pub crossfade: Crossfade,
    pub replaygain: Replaygain,
    pub unity_volume: UnityVolume,
    pub device: Option<DeviceName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnityVolume {
    Pinned,
    #[default]
    Free,
}
