use std::time::Duration;

use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, revision::Revision, speed::Speed},
    message::AudioEvent,
    update::machine::LoopCmd,
};

use crate::{
    deck::{
        job::AudioJob,
        source::{PreloadMode, TrackSource},
    },
    engine::message::AudioMessage,
    gain::Gain,
};

pub(crate) type AudioLoopCmd =
    LoopCmd<EngineEffect, AudioJob, AudioMessage, AudioEvent>;

#[derive(Debug, PartialEq)]
pub enum EngineEffect {
    Silence,
    Open {
        device: OutputDevice,
        speed: Speed,
    },
    StartLoad(Speed),
    StartHandover(Speed),
    Decode,
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
    ClearStaged,
    Promote(Gain),
    Report,
    Advance(Gain),
    Stage(TrackSource),
    Attach {
        track_source: TrackSource,
        preload_mode: PreloadMode,
    },
    TakeSignals(Revision),
}
