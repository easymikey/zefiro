use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioError,
    AudioEvent,
    Playback,
    domain::{ListedDevice, OutputDevice, Speed},
};

use crate::{
    deck::{DeviceOpened, source::PreloadRequest},
    engine::phase::CurrentTrack,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SinkRole {
    Primary,
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Preload {
    Gapless(PathBuf),
    Crossfade(CurrentTrack),
}

#[derive(Debug, Clone)]
pub(crate) enum EngineMessage {
    Cmd(AudioCmd),
    Opened(Result<DeviceOpened, AudioError>),
    Decoded(Result<Option<Duration>, AudioError>),
    Preloaded(Result<Preload, AudioError>),
    Failed(AudioError),
    Finished(SinkRole),
    Cued,
    Ramped(SinkRole),
    DevicesListed(Vec<ListedDevice>),
}

#[derive(Debug, Default, PartialEq)]
pub(crate) enum EngineEffect {
    #[default]
    Nothing,
    Batch(Vec<EngineEffect>),
    Send(AudioEvent),
    Mute(AudioError),
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
    Start {
        volume: f32,
        total: Option<Duration>,
    },
    Resume {
        volume: f32,
        position: Duration,
        playback: Playback,
    },
    Play,
    Pause,
    Seek(Duration),
    SetVolume(f32),
    Arm {
        cue: Option<Duration>,
    },
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
    Preload(PreloadRequest),
    RestartGapless(PathBuf),
    Promote {
        volume: f32,
    },
    ListDevices,
    Report,
    Advance,
}
