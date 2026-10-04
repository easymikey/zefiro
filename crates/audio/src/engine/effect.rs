use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioError,
    Cmds,
    Playback,
    domain::{ListedDevice, OutputDevice, Revision, Speed},
};

use crate::{
    deck::{
        DeviceOpened,
        envelope::Signals,
        event::DeckEvent,
        job::AudioJob,
        source::{PreloadMode, TrackSource},
    },
    engine::phase::CurrentTrack,
    gain::Gain,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkRole {
    Primary,
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreloadKind {
    Gapless(PathBuf),
    Crossfade(CurrentTrack),
}

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

#[derive(Debug, PartialEq)]
pub enum EngineEffect {
    Mute,
    Open {
        device: OutputDevice,
        speed: Speed,
    },
    StartLoad {
        path: PathBuf,
        speed: Speed,
    },
    StartHandover {
        path: PathBuf,
        speed: Speed,
    },
    Decode(PathBuf),
    Start(Gain),
    Resume {
        gain: Gain,
        position: Duration,
        playback: Playback,
    },
    Play,
    Pause,
    Seek(Duration),
    SetGain(Gain),
    Arm(Option<Duration>),
    Crossfade {
        length: Duration,
        incoming: Gain,
    },
    CancelCrossfade,
    Ramp {
        length: Duration,
        playing: Gain,
    },
    DropOutgoing,
    SetSpeed(Speed),
    Clear(Speed),
    Preload {
        path: PathBuf,
        mode: PreloadMode,
    },
    RestartGapless(PathBuf),
    Promote(Gain),
    Run(AudioJob),
    Report,
    Advance,
    Stage(TrackSource),
    Attach(TrackSource),
    TakeSignals(Revision),
}
