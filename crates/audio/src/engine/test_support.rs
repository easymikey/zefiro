use std::time::Duration;

use kernel::{
    AudioCmd,
    AudioError,
    Bounded,
    DecodeError,
    Playback,
    TrackLoad,
    domain::{
        Crossfade,
        DeviceName,
        OutputDevice,
        ReplayGain,
        Revision,
        Speed,
        StreamError,
    },
    update::Machine,
};

use crate::{
    EngineConfig,
    deck::{DeviceChoice, DeviceOpened, source::PreloadRequest},
    engine::{
        effect::{EngineEffect, EngineMessage, Preload},
        phase::{
            CurrentTrack,
            Handover,
            Incoming,
            Loading,
            Next,
            Phase,
            Playing,
            Resume,
        },
        state::{Engine, Live, Muted, PerformedRevisions},
    },
};

pub(crate) const TOTAL: Duration = Duration::from_secs(100);
pub(crate) const PRELOAD_TOTAL: Duration = Duration::from_secs(90);
pub(crate) const CROSSFADE_SECONDS: u64 = 10;

pub(crate) fn seconds(count: u64) -> Duration {
    Duration::from_secs(count)
}

pub(crate) fn crossfade(count: u64) -> Crossfade {
    Crossfade::clamped(seconds(count))
}

pub(crate) fn config() -> EngineConfig {
    EngineConfig {
        crossfade: crossfade(0),
        replay_gain: ReplayGain::Off,
        device: OutputDevice::SystemDefault,
    }
}

pub(crate) fn config_on(device: &str) -> EngineConfig {
    EngineConfig {
        device: OutputDevice::Named(DeviceName::new(device.to_string()).unwrap()),
        ..config()
    }
}

pub(crate) fn error() -> AudioError {
    AudioError::Stream {
        reason: "no output device available".to_string(),
    }
}

pub(crate) fn decode_error() -> AudioError {
    AudioError::Decode {
        path: "/a".into(),
        kind: DecodeError::Unsupported,
    }
}

pub(crate) fn output_lost() -> AudioError {
    AudioError::OutputLost {
        kind: StreamError::DeviceGone,
    }
}

pub(crate) fn failed() -> EngineMessage {
    EngineMessage::Failed(output_lost())
}

pub(crate) fn muted() -> Engine {
    Engine::Muted(Muted {
        error: error(),
        config: config(),
        pending: None,
        speed: Speed::default(),
    })
}

pub(crate) fn waiting_for(path: &str) -> Engine {
    Engine::Muted(Muted {
        pending: Some(TrackLoad {
            path: path.into(),
            gain: None,
            revision: first(),
        }),
        error: error(),
        config: config(),
        speed: Speed::default(),
    })
}

pub(crate) fn live() -> Live {
    Live::new(config(), Speed::default())
}

pub(crate) fn track_a() -> CurrentTrack {
    CurrentTrack {
        total: Some(TOTAL),
        gain: None,
        path: "/a".into(),
    }
}

pub(crate) fn track_b() -> CurrentTrack {
    CurrentTrack {
        total: Some(PRELOAD_TOTAL),
        gain: None,
        path: "/b".into(),
    }
}

pub(crate) fn playing_track(current: CurrentTrack) -> Phase {
    Phase::Playing(Playing::new(current))
}

pub(crate) fn loading_track(path: &str) -> Loading {
    Loading {
        path: path.into(),
        gain: None,
        after_load: None,
    }
}

pub(crate) fn playing() -> Live {
    Live {
        phase: playing_track(track_a()),
        ..live()
    }
}

pub(crate) fn loading() -> Live {
    Live {
        phase: Phase::Loading(loading_track("/a")),
        ..live()
    }
}

pub(crate) fn resuming() -> Live {
    Live {
        phase: Phase::Loading(Loading {
            path: "/a".into(),
            gain: None,
            after_load: Some(Resume {
                position: seconds(5),
                playback: Playback::Paused,
                total: Some(TOTAL),
            }),
        }),
        config: config_on("usb"),
        ..live()
    }
}

pub(crate) fn live_with_crossfade(seconds: u64) -> Live {
    Live {
        config: EngineConfig {
            crossfade: crossfade(seconds),
            ..config()
        },
        ..live()
    }
}

pub(crate) fn playing_with_crossfade() -> Live {
    Live {
        config: EngineConfig {
            crossfade: crossfade(CROSSFADE_SECONDS),
            ..config()
        },
        ..playing()
    }
}

pub(crate) fn handing_over(incoming: Incoming) -> Live {
    Live {
        phase: Phase::Handover(Handover { incoming }),
        ..playing_with_crossfade()
    }
}

pub(crate) fn handed_over_to_b() -> Live {
    handing_over(Incoming::Playing(track_b()))
}

pub(crate) fn crossfading_idle() -> Live {
    Live {
        phase: Phase::Playing(Playing {
            next: Next::Crossfading {
                preload: track_b(),
                fading: false,
            },
            ..Playing::new(track_a())
        }),
        config: EngineConfig {
            crossfade: crossfade(CROSSFADE_SECONDS),
            ..config()
        },
        ..live()
    }
}

pub(crate) fn crossfading_mid_ramp() -> Live {
    Live {
        phase: Phase::Playing(Playing {
            next: Next::Crossfading {
                preload: track_b(),
                fading: true,
            },
            ..Playing::new(track_a())
        }),
        ..crossfading_idle()
    }
}

pub(crate) fn promoted(crossfade: Crossfade) -> Live {
    Live {
        phase: playing_track(track_b()),
        config: EngineConfig {
            crossfade,
            ..config()
        },
        ..live()
    }
}

pub(crate) fn cmd(cmd: AudioCmd) -> EngineMessage {
    EngineMessage::Cmd(cmd)
}

pub(crate) fn first() -> Revision {
    Revision::default().next()
}

pub(crate) fn second() -> Revision {
    first().next()
}

pub(crate) fn load_at(path: &str, revision: Revision) -> EngineMessage {
    cmd(AudioCmd::Load(TrackLoad {
        path: path.into(),
        gain: None,
        revision,
    }))
}

pub(crate) fn load(path: &str) -> EngineMessage {
    load_at(path, first())
}

pub(crate) fn preload_at(path: &str, revision: Revision) -> EngineMessage {
    cmd(AudioCmd::Preload(TrackLoad {
        path: path.into(),
        gain: None,
        revision,
    }))
}

pub(crate) fn preload(path: &str) -> EngineMessage {
    preload_at(path, first())
}

pub(crate) fn loaded_at(live: Live, revision: Revision) -> Live {
    Live {
        performed: PerformedRevisions {
            load: revision,
            ..live.performed
        },
        ..live
    }
}

pub(crate) fn awaiting(live: Live, path: &str) -> Live {
    let Phase::Playing(playing) = live.phase else {
        panic!(
            "only a playing engine awaits a preload, got {:?}",
            live.phase
        );
    };
    Live {
        phase: Phase::Playing(Playing {
            preloading: Some(path.into()),
            ..playing
        }),
        ..live
    }
}

pub(crate) fn installed(preload: CurrentTrack) -> EngineMessage {
    EngineMessage::Preloaded(Ok(Preload::Crossfade(preload)))
}

pub(crate) fn gapless_preload(path: &str) -> EngineEffect {
    EngineEffect::Preload(PreloadRequest::Gapless(path.into()))
}

pub(crate) fn crossfade_preload(path: &str) -> EngineEffect {
    EngineEffect::Preload(PreloadRequest::Crossfade {
        path: path.into(),
        gain: None,
        speed: Speed::default(),
    })
}

pub(crate) fn preloaded_at(live: Live, revision: Revision) -> Live {
    Live {
        performed: PerformedRevisions {
            incoming: revision,
            ..live.performed
        },
        ..live
    }
}

pub(crate) fn set_crossfade(seconds: u64) -> EngineMessage {
    cmd(AudioCmd::SetCrossfade(crossfade(seconds)))
}

pub(crate) fn opened(
    device: OutputDevice,
    position: Duration,
    playback: Playback,
) -> EngineMessage {
    EngineMessage::Opened(Ok(DeviceOpened {
        device,
        position,
        playback,
        opened: DeviceChoice::Requested,
    }))
}

pub(crate) fn fell_back(position: Duration, playback: Playback) -> EngineMessage {
    EngineMessage::Opened(Ok(DeviceOpened {
        device: OutputDevice::SystemDefault,
        position,
        playback,
        opened: DeviceChoice::FellBack,
    }))
}

pub(crate) struct Cell {
    pub(crate) next: Engine,
    pub(crate) effect: EngineEffect,
}

pub(crate) fn assert_cell(start: Engine, message: EngineMessage, moved: Cell) {
    let mut state = start;
    let effect = state.transition(message).unwrap();
    let Cell {
        next,
        effect: expected,
    } = moved;
    assert_eq!(state, next);
    assert_eq!(effect, expected);
}
