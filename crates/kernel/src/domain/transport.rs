use std::time::Duration;

use crate::domain::{AbLoop, Bounded, Percent, SleepTimer, Speed};

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
    Lost {
        kind: StreamError,
    },
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

pub(crate) const SEEK_SMALL: i64 = 5;
pub(crate) const SEEK_MEDIUM: i64 = 10;
pub(crate) const SEEK_LARGE: i64 = 30;
