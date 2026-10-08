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
        source::{DecodedTrack, PreloadMode},
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
    ClearStaged,
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
    SetFadeStart(Option<Duration>),
    Crossfade {
        duration: Duration,
        incoming: Gain,
    },
    CancelCrossfade,
    Ramp {
        duration: Duration,
        current: Gain,
    },
    DropOutgoing,
    SetSpeed(Speed),
    DropPreload,
    Promote(Gain),
    Report,
    Advance(Gain),
    Stage(DecodedTrack),
    Attach {
        decoded_track: DecodedTrack,
        preload_mode: PreloadMode,
    },
    TakeSignals(Revision),
}
