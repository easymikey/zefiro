use std::{fmt, time::Duration};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    cmd::{AudioCmd, Cmds, Playback},
    domain::{
        device::{ListedDevice, OutputDevice},
        revision::Revision,
        transport::OutputError,
    },
    message::AudioError,
};

use crate::{
    deck::{
        event::DeckEvent,
        source::{PreloadMode, TrackDecoder},
    },
    error::Error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkRole {
    Current,
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceOpened {
    pub(crate) device: OutputDevice,
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals(pub(crate) u8);

impl Signals {
    pub(crate) const FINISHED: Self = Self(1);
    pub(crate) const FADE_START: Self = Self(2);
    pub(crate) const RAMPED: Self = Self(4);

    #[must_use]
    pub(crate) fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}

pub enum AudioMessage {
    Deck(DeckEvent),
    Decoded {
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    },
    Preloaded {
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    },
    DevicesListed(Result<Vec<ListedDevice>, AudioError>),
    SignalsTaken {
        role: SinkRole,
        signals: Signals,
    },
    Engine(EngineMessage),
}

impl fmt::Debug for AudioMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioMessage::Deck(event) => f.debug_tuple("Deck").field(event).finish(),
            AudioMessage::Decoded { revision, .. } => f
                .debug_struct("Decoded")
                .field("revision", revision)
                .finish_non_exhaustive(),
            AudioMessage::Preloaded { revision, .. } => f
                .debug_struct("Preloaded")
                .field("revision", revision)
                .finish_non_exhaustive(),
            AudioMessage::DevicesListed(listed) => {
                f.debug_tuple("DevicesListed").field(listed).finish()
            }
            AudioMessage::SignalsTaken { role, signals } => f
                .debug_struct("SignalsTaken")
                .field("role", role)
                .field("signals", signals)
                .finish(),
            AudioMessage::Engine(message) => {
                f.debug_tuple("Engine").field(message).finish()
            }
        }
    }
}

#[derive(Debug)]
pub enum EngineMessage {
    Cmds(Cmds<AudioCmd>),
    Reported(Option<Duration>),
    Error(AudioError),
    OutputLost(OutputError),
    Opened(DeviceOpened),
    NotFound,
    Decoded(Option<Duration>),
    Attached {
        revision: Revision,
        preload_mode: PreloadMode,
        duration: Option<Duration>,
    },
    Finished(SinkRole),
    FadeStartReached,
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
        callback_sender: &Sender<AudioMessage>,
    ) -> Result<(), TrySendError<AudioMessage>> {
        match callback_sender.try_send(AudioMessage::Deck(self)) {
            Err(TrySendError::Disconnected(_)) => Ok(()),
            sent => sent,
        }
    }
}

pub(crate) fn signalled(role: SinkRole, signals: Signals) -> Vec<EngineMessage> {
    [
        (signals.contains(Signals::FADE_START) && role == SinkRole::Current)
            .then_some(EngineMessage::FadeStartReached),
        signals
            .contains(Signals::RAMPED)
            .then_some(EngineMessage::Ramped(role)),
        signals
            .contains(Signals::FINISHED)
            .then_some(EngineMessage::Finished(role)),
    ]
    .into_iter()
    .flatten()
    .collect()
}
