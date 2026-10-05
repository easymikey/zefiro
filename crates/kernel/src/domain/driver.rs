use std::time::Duration;

use strum::{Display, IntoStaticStr};

use crate::domain::time::Moment;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DriverName {
    Audio,
    Library,
    Config,
    Macos,
}

impl DriverName {
    pub const ALL: [DriverName; 4] = [
        DriverName::Audio,
        DriverName::Library,
        DriverName::Config,
        DriverName::Macos,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    #[error("panicked")]
    Panicked,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DriverStatus {
    #[default]
    Running,
    Dead(DriverError),
    Stopped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restarts(Vec<Moment>);

impl Restarts {
    pub fn record(&mut self, now: Moment) {
        self.0.push(now);
        if self.0.len() > usize::from(u8::MAX) {
            self.0.remove(0);
        }
    }

    #[must_use]
    pub(crate) fn within(&self, window: Duration, now: Moment) -> usize {
        self.0
            .iter()
            .filter(|moment| now.elapsed_since(**moment) < window)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverRecord {
    pub status: DriverStatus,
    pub restarts: Restarts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drivers([DriverRecord; 4]);

impl Default for Drivers {
    fn default() -> Self {
        Self(DriverName::ALL.map(|_| DriverRecord {
            status: DriverStatus::default(),
            restarts: Restarts::default(),
        }))
    }
}

impl Drivers {
    #[must_use]
    pub fn status(&self, driver: DriverName) -> &DriverStatus {
        &self.record(driver).status
    }

    #[must_use]
    pub fn record(&self, driver: DriverName) -> &DriverRecord {
        let [audio, library, config, macos] = &self.0;
        match driver {
            DriverName::Audio => audio,
            DriverName::Library => library,
            DriverName::Config => config,
            DriverName::Macos => macos,
        }
    }

    pub fn record_mut(&mut self, driver: DriverName) -> &mut DriverRecord {
        let [audio, library, config, macos] = &mut self.0;
        match driver {
            DriverName::Audio => audio,
            DriverName::Library => library,
            DriverName::Config => config,
            DriverName::Macos => macos,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::driver::{DriverName, DriverStatus, Drivers};

    #[test]
    fn every_driver_indexes_its_own_record() {
        for driver in DriverName::ALL {
            let mut drivers = Drivers::default();
            drivers.record_mut(driver).status = DriverStatus::Stopped;

            for other in DriverName::ALL {
                let expected = if other == driver {
                    &DriverStatus::Stopped
                } else {
                    &DriverStatus::Running
                };
                assert_eq!(drivers.status(other), expected);
            }
        }
    }
}
