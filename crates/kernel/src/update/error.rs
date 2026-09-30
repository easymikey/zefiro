use crate::{
    domain::Driver,
    update::{driver::DriverStatusError, overlay::OverlayError, player::PlayerError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    Player(PlayerError),
    Overlay(OverlayError),
    Driver(Driver, DriverStatusError),
}

impl From<PlayerError> for UpdateError {
    fn from(rejection: PlayerError) -> Self {
        Self::Player(rejection)
    }
}

impl From<OverlayError> for UpdateError {
    fn from(rejection: OverlayError) -> Self {
        Self::Overlay(rejection)
    }
}

impl From<std::convert::Infallible> for UpdateError {
    fn from(never: std::convert::Infallible) -> Self {
        match never {}
    }
}
