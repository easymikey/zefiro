use crate::{
    domain::Driver,
    update::{driver::DriverStatusError, overlay::OverlayError, player::PlayerError},
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error("player: {0}")]
    Player(PlayerError),
    #[error("overlay: {0}")]
    Overlay(OverlayError),
    #[error("{0} driver: {1}")]
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
