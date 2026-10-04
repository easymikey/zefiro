use std::time::Duration;

use kernel::{
    cmd::{AudioCmd, Cmds},
    domain::device::ListedDevice,
    message::AudioError,
};

use crate::{
    deck::{DeviceOpened, envelope::Signals, event::DeckEvent},
    engine::effect::{PreloadKind, SinkRole},
};

#[derive(Debug)]
pub enum AudioMessage {
    Deck(DeckEvent),
    Cmds(Cmds<AudioCmd>),
    Reported(Option<Duration>),
    Error(AudioError),
    Opened(DeviceOpened),
    Decoded(Option<Duration>),
    Preloaded(PreloadKind),
    Finished(SinkRole),
    Cued,
    Ramped(SinkRole),
    DevicesListed(Vec<ListedDevice>),
    SignalsTaken { role: SinkRole, signals: Signals },
}

#[derive(Debug)]
pub(crate) enum EngineMessage {
    Cmds(Cmds<AudioCmd>),
    Reported(Option<Duration>),
    Error(AudioError),
    Opened(DeviceOpened),
    Decoded(Option<Duration>),
    Preloaded(PreloadKind),
    Finished(SinkRole),
    Cued,
    Ramped(SinkRole),
    DevicesListed(Vec<ListedDevice>),
}

impl From<Cmds<AudioCmd>> for AudioMessage {
    fn from(cmds: Cmds<AudioCmd>) -> Self {
        AudioMessage::Cmds(cmds)
    }
}
