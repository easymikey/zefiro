use std::time::Duration;

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    cmd::{AudioCmd, Cmds, Playback},
    domain::{
        device::{ListedDevice, OutputDevice},
        revision::Revision,
        transport::StreamError,
    },
    message::AudioError,
};

use crate::deck::{event::DeckEvent, source::PreloadMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkRole {
    Primary,
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceOpened {
    pub(crate) device: OutputDevice,
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
    pub(crate) opened: DeviceChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceChoice {
    FellBack,
    Requested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals(pub(crate) u8);

impl Signals {
    pub(crate) const FINISHED: Self = Self(1);
    pub(crate) const CUED: Self = Self(2);
    pub(crate) const RAMPED: Self = Self(4);

    #[must_use]
    pub(crate) fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}

#[derive(Debug)]
pub enum AudioMessage {
    Deck(DeckEvent),
    SignalsTaken { role: SinkRole, signals: Signals },
    Engine(EngineMessage),
}

#[derive(Debug)]
pub enum EngineMessage {
    Cmds(Cmds<AudioCmd>),
    Reported(Option<Duration>),
    Error(AudioError),
    OutputLost(StreamError),
    Opened(DeviceOpened),
    Decoded(Option<Duration>),
    Attached {
        revision: Revision,
        preload_mode: PreloadMode,
        duration: Option<Duration>,
    },
    Finished(SinkRole),
    Cued,
    Ramped(SinkRole),
    DevicesListed(Vec<ListedDevice>),
}

impl From<Cmds<AudioCmd>> for AudioMessage {
    fn from(cmds: Cmds<AudioCmd>) -> Self {
        AudioMessage::Engine(EngineMessage::Cmds(cmds))
    }
}

impl From<EngineMessage> for AudioMessage {
    fn from(message: EngineMessage) -> Self {
        AudioMessage::Engine(message)
    }
}

impl DeckEvent {
    pub(crate) fn wake(
        self,
        sender: &Sender<AudioMessage>,
    ) -> Result<(), TrySendError<AudioMessage>> {
        match sender.try_send(AudioMessage::Deck(self)) {
            Err(TrySendError::Disconnected(_)) => Ok(()),
            sent => sent,
        }
    }
}
