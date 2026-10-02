use strum::{Display, IntoStaticStr};

use crate::domain::Restarts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Driver {
    Audio,
    Library,
    Config,
    Macos,
}

impl Driver {
    pub const ALL: [Driver; 4] = [
        Driver::Audio,
        Driver::Library,
        Driver::Config,
        Driver::Macos,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Driver::Audio => 0,
            Driver::Library => 1,
            Driver::Config => 2,
            Driver::Macos => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    #[error("panicked: {text}")]
    Panicked { text: String },
}

impl DriverError {
    #[must_use]
    pub fn panicked(text: impl Into<String>) -> Self {
        DriverError::Panicked { text: text.into() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DriverStatus {
    #[default]
    Running,
    Dead(DriverError),
    Stopped,
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
        Self(Driver::ALL.map(|_| DriverRecord {
            status: DriverStatus::default(),
            restarts: Restarts::default(),
        }))
    }
}

impl Drivers {
    #[must_use]
    pub fn status(&self, driver: Driver) -> &DriverStatus {
        &self.record(driver).status
    }

    #[must_use]
    pub fn record(&self, driver: Driver) -> &DriverRecord {
        let [audio, library, config, macos] = &self.0;
        match driver {
            Driver::Audio => audio,
            Driver::Library => library,
            Driver::Config => config,
            Driver::Macos => macos,
        }
    }

    pub fn record_mut(&mut self, driver: Driver) -> &mut DriverRecord {
        let [audio, library, config, macos] = &mut self.0;
        match driver {
            Driver::Audio => audio,
            Driver::Library => library,
            Driver::Config => config,
            Driver::Macos => macos,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{Driver, DriverStatus, Drivers};

    #[test]
    fn every_driver_indexes_its_own_record() {
        for (position, driver) in Driver::ALL.into_iter().enumerate() {
            assert_eq!(driver.index(), position);

            let mut drivers = Drivers::default();
            drivers.record_mut(driver).status = DriverStatus::Stopped;

            for other in Driver::ALL {
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
