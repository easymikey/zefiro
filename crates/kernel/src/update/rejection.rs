use crate::{
    domain::Driver,
    message::EngineRejection,
    update::{
        driver::DriverRejection,
        machine::Never,
        overlay::OverlayRejection,
        player::PlayerRejection,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    Player(PlayerRejection),
    Overlay(OverlayRejection),
    Driver(Driver, DriverRejection),
    Engine(EngineRejection),
}

impl From<PlayerRejection> for Rejection {
    fn from(rejection: PlayerRejection) -> Self {
        Self::Player(rejection)
    }
}

impl From<OverlayRejection> for Rejection {
    fn from(rejection: OverlayRejection) -> Self {
        Self::Overlay(rejection)
    }
}

impl From<Never> for Rejection {
    fn from(never: Never) -> Self {
        match never {}
    }
}
