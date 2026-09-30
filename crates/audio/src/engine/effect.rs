use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioError,
    AudioEvent,
    Playback,
    domain::{ListedDevice, OutputDevice, Speed},
};

use crate::deck::DeviceOpened;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    Primary,
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Preload {
    Gapless(PathBuf),
    Crossfade(PreloadedTrack),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreloadedTrack {
    pub(crate) path: PathBuf,
    pub(crate) gain: Option<f32>,
    pub(crate) total: Option<Duration>,
}

#[derive(Debug, Clone)]
pub(crate) enum EngineMessage {
    Cmd(AudioCmd),
    Opened(Result<DeviceOpened, AudioError>),
    Decoded(Result<Option<Duration>, AudioError>),
    Preloaded(Result<Preload, AudioError>),
    Failed(AudioError),
    Retiring { from: f32 },
    Finished(Slot),
    Cued,
    Ramped(Slot),
    DevicesListed(Result<Vec<ListedDevice>, AudioError>),
}

pub(crate) fn devices_event(
    devices: Result<Vec<ListedDevice>, AudioError>,
) -> AudioEvent {
    devices.map_or_else(
        |_error| AudioEvent::DevicesListed(Vec::new()),
        AudioEvent::DevicesListed,
    )
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
    StartFade {
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
        paused: Playback,
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
    Unfade,
    Ramp {
        length: Duration,
        playing: f32,
    },
    DropOutgoing,
    SetSpeed(Speed),
    Clear,
    PreloadGapless(PathBuf),
    PreloadCrossfade {
        path: PathBuf,
        gain: Option<f32>,
        speed: Speed,
    },
    RestartGapless(PathBuf),
    Promote {
        volume: f32,
    },
    ListDevices,
    Report,
    Advance,
}
