use std::time::Duration;

use crate::domain::{AbLoop, Bounded, Percent, SleepTimer, Speed};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    DeviceGone,
    Backend,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            StreamError::DeviceGone => "the device is gone",
            StreamError::Backend => "an audio backend error",
        };
        formatter.write_str(label)
    }
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
    pub ab: Option<AbLoop>,
    pub output: Output,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            volume: Percent::clamped(50),
            speed: Speed::default(),
            sleep: None,
            ab: None,
            output: Output::Ready,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SeekSteps {
    pub small: i64,
    pub medium: i64,
    pub large: i64,
}

impl Default for SeekSteps {
    fn default() -> Self {
        Self {
            small: 5,
            medium: 10,
            large: 30,
        }
    }
}
