use strum::{Display, IntoStaticStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Driver {
    Audio,
    Library,
    Config,
    Macos,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverFailure {
    #[error("panicked: {0}")]
    Panicked(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DriverStatus {
    #[default]
    Running,
    Dead(DriverFailure),
    Stopped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drivers {
    pub audio: DriverStatus,
    pub library: DriverStatus,
    pub config: DriverStatus,
    pub macos: DriverStatus,
}

impl Drivers {
    #[must_use]
    pub fn status(&self, driver: Driver) -> &DriverStatus {
        match driver {
            Driver::Audio => &self.audio,
            Driver::Library => &self.library,
            Driver::Config => &self.config,
            Driver::Macos => &self.macos,
        }
    }

    pub(crate) fn status_mut(&mut self, driver: Driver) -> &mut DriverStatus {
        match driver {
            Driver::Audio => &mut self.audio,
            Driver::Library => &mut self.library,
            Driver::Config => &mut self.config,
            Driver::Macos => &mut self.macos,
        }
    }
}
