use std::time::Duration;

use crate::domain::{AbLoop, Bounded, Percent, SleepTimer, Speed};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFault {
    DeviceGone,
    Backend,
}

impl std::fmt::Display for OutputFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            OutputFault::DeviceGone => "the device is gone",
            OutputFault::Backend => "an audio backend error",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Output {
    #[default]
    Ready,
    Lost {
        fault: OutputFault,
    },
}

#[derive(Debug, Clone)]
pub struct Transport {
    pub volume: Percent,
    pub speed: Speed,
    pub sleep: Option<SleepTimer>,
    pub ab: Option<AbLoop>,
    pub preload_lead: Duration,
    pub output: Output,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            volume: Percent::clamped(50),
            speed: Speed::default(),
            sleep: None,
            ab: None,
            preload_lead: Duration::from_secs(10),
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
