use std::time::Duration;

use crate::domain::{
    bounded::Bounded,
    percent::Percent,
    player::AbLoop,
    sleep::SleepTimer,
    speed::Speed,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StreamError {
    #[error("the device is gone")]
    DeviceGone,
    #[error("an audio backend error")]
    Backend,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Output {
    #[default]
    Ready,
    Lost(StreamError),
}

pub const PRELOAD_LEAD: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct Transport {
    pub volume: Percent,
    pub speed: Speed,
    pub sleep: Option<SleepTimer>,
    pub ab_loop: Option<AbLoop>,
    pub output: Output,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            volume: Percent::clamped(50),
            speed: Speed::default(),
            sleep: None,
            ab_loop: None,
            output: Output::Ready,
        }
    }
}

pub const SEEK_SMALL: Duration = Duration::from_secs(5);
pub const SEEK_MEDIUM: Duration = Duration::from_secs(10);
pub const SEEK_LARGE: Duration = Duration::from_secs(30);
