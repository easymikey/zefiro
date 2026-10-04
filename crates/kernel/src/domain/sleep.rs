use std::time::Duration;

use crate::domain::index::PresetIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepTimer {
    pub preset_index: PresetIndex,
    pub delay: Duration,
}
