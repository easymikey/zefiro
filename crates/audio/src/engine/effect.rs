use std::{path::PathBuf, time::Duration};

use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, revision::Revision, speed::Speed},
};

use crate::{
    deck::{
        job::AudioJob,
        source::{PreloadMode, TrackSource},
    },
    gain::Gain,
};

#[derive(Debug, PartialEq)]
pub enum EngineEffect {
    Silence,
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
    Preload(PreloadMode),
    RestartGapless(PathBuf),
    Promote(Gain),
    Run(AudioJob),
    Report,
    Advance(Gain),
    Stage(TrackSource),
    Attach(TrackSource),
    TakeSignals(Revision),
}
