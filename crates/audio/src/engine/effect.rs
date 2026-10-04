use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioError,
    Cmds,
    Playback,
    domain::{ListedDevice, OutputDevice, Speed},
};

use crate::{
    deck::{
        AudioJob,
        DeckEvent,
        DeviceOpened,
        Revision,
        envelope::Signals,
        source::{PreloadMode, TrackSource},
    },
    engine::phase::CurrentTrack,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkRole {
    Primary,
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Preload {
    Gapless(PathBuf),
    Crossfade(CurrentTrack),
}

#[derive(Debug)]
pub enum AudioMessage {
    Deck(DeckEvent),
    Cmds(Cmds<AudioCmd>),
    Reported(Option<Duration>),
    Error(AudioError),
    Opened(Result<DeviceOpened, AudioError>),
    Decoded(Result<Option<Duration>, AudioError>),
    Preloaded(Result<Preload, AudioError>),
    Finished(SinkRole),
    Cued,
    Ramped(SinkRole),
    DevicesListed(Result<Vec<ListedDevice>, AudioError>),
    SignalsTaken { role: SinkRole, signals: Signals },
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
    Start(f32),
    Resume {
        volume: f32,
        position: Duration,
        playback: Playback,
    },
    Play,
    Pause,
    Seek(Duration),
    SetVolume(f32),
    Arm(Option<Duration>),
    Crossfade {
        length: Duration,
        incoming: f32,
    },
    CancelCrossfade,
    Ramp {
        length: Duration,
        playing: f32,
    },
    DropOutgoing,
    SetSpeed(Speed),
    Clear,
    Preload {
        path: PathBuf,
        mode: PreloadMode,
    },
    RestartGapless(PathBuf),
    Promote(f32),
    Run(AudioJob),
    Report,
    Advance,
    Stage(TrackSource),
    Attach(TrackSource),
    TakeSignals(Revision),
}

impl EngineEffect {
    pub fn into_job(self) -> Result<AudioJob, Self> {
        if let EngineEffect::Run(job) = self {
            Ok(job)
        } else {
            Err(self)
        }
    }
}
