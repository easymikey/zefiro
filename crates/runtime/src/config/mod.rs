pub(crate) mod disk;
pub(crate) mod driver;
pub(crate) mod machine;
pub(crate) mod reload;
pub(crate) mod save_queue;
pub(crate) mod seen;
pub(crate) mod session;
pub(crate) mod watch;
pub(crate) mod write;

use std::{path::PathBuf, time::Duration};

#[must_use]
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub config: PathBuf,
    pub appearance: PathBuf,
    pub themes: PathBuf,
    pub theme: Option<String>,
    pub seen: SeenTexts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenTexts {
    pub appearance: Option<String>,
    pub theme: Option<String>,
    pub config: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConfigTiming {
    pub(crate) save_debounce: Duration,
}

impl Default for ConfigTiming {
    fn default() -> Self {
        Self {
            save_debounce: Duration::from_millis(200),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::config::ConfigTiming;

    #[test]
    fn the_stock_timing_debounces_a_save_by_two_hundred_milliseconds() {
        let timing = ConfigTiming::default();

        assert_eq!(timing.save_debounce, Duration::from_millis(200));
    }
}
