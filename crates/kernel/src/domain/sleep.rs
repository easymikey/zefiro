use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepTimer {
    pub preset_index: usize,
    pub delay: Duration,
}
