use std::path::PathBuf;

use kernel::{
    AudioError,
    AudioEvent,
    domain::{Bounded, OutputDevice, Percent, Revision, Speed},
    update::Rejected,
};

use crate::{
    EngineConfig,
    deck::DeviceChoice,
    engine::{crossfade::effective_volume, effect::EngineEffect, phase::Phase},
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Engine {
    Muted(Muted),
    Live(Live),
}

impl Default for Engine {
    fn default() -> Self {
        Engine::Live(Live::new(EngineConfig::default()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Muted {
    pub(crate) error: AudioError,
    pub(crate) config: EngineConfig,
    pub(crate) pending: Option<TrackRequest>,
    pub(crate) mix: Mix,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mix {
    pub(crate) speed: Speed,
    pub(crate) volume: Percent,
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            speed: Speed::default(),
            volume: Percent::clamped(Percent::MAX),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrackRequest {
    pub(crate) path: PathBuf,
    pub(crate) gain: Option<f32>,
    pub(crate) revision: Revision,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Live {
    pub(crate) phase: Phase,
    pub(crate) speed: Speed,
    pub(crate) volume: Percent,
    pub(crate) config: EngineConfig,
    pub(crate) performed: PerformedRevisions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PerformedRevisions {
    pub(crate) load: Revision,
    pub(crate) incoming: Revision,
}

impl Live {
    #[must_use]
    pub(crate) fn new(config: EngineConfig) -> Self {
        Self::with_mix(config, Mix::default())
    }

    #[must_use]
    pub(crate) fn with_mix(config: EngineConfig, mix: Mix) -> Self {
        Self {
            phase: Phase::Idle,
            speed: mix.speed,
            volume: mix.volume,
            config,
            performed: PerformedRevisions::default(),
        }
    }

    pub(crate) fn volume(&self) -> f32 {
        let gain = self.phase.current().and_then(|current| current.gain);
        effective_volume(&self.config, gain, self.volume.ratio())
    }
}

pub(crate) fn announce(
    opened: DeviceChoice,
    device: OutputDevice,
    effect: EngineEffect,
) -> EngineEffect {
    match opened {
        DeviceChoice::Requested => effect,
        DeviceChoice::FellBack => announce_fallback(device, effect),
    }
}

fn announce_fallback(device: OutputDevice, then: EngineEffect) -> EngineEffect {
    let told = EngineEffect::Send(AudioEvent::DeviceFellBack(device));
    if matches!(then, EngineEffect::Nothing) {
        return told;
    }
    EngineEffect::Batch(vec![told, then])
}

pub(crate) fn reported(effect: EngineEffect) -> EngineEffect {
    if matches!(effect, EngineEffect::Nothing) {
        return EngineEffect::Report;
    }
    if let EngineEffect::Batch(mut steps) = effect {
        steps.push(EngineEffect::Report);
        return EngineEffect::Batch(steps);
    }
    EngineEffect::Batch(vec![effect, EngineEffect::Report])
}

pub(crate) enum Transition {
    Next(Engine, EngineEffect),
    Rejected(Rejected<Engine>),
}

impl From<(Engine, EngineEffect)> for Transition {
    fn from(moved: (Engine, EngineEffect)) -> Self {
        let (engine, effect) = moved;
        Transition::Next(engine, effect)
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::time::Duration;

    use kernel::{
        AudioCmd,
        AudioError,
        Bounded,
        DecodeError,
        Playback,
        domain::{
            Crossfade,
            DeviceName,
            OutputDevice,
            Replaygain,
            Revision,
            StreamError,
        },
    };

    use crate::{
        EngineConfig,
        UnityVolume,
        deck::{DeviceChoice, DeviceOpened},
        engine::{
            effect::{EngineMessage, Preload, PreloadedTrack},
            phase::{
                AfterLoad,
                CurrentTrack,
                Fade,
                Handover,
                Incoming,
                Loading,
                Next,
                Outgoing,
                Phase,
                Playing,
            },
            state::{Engine, Live, Mix, Muted, PerformedRevisions, TrackRequest},
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
            replaygain: Replaygain::Off,
            unity_volume: UnityVolume::Free,
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
            mix: Mix::default(),
        })
    }

    pub(crate) fn waiting_for(path: &str) -> Engine {
        Engine::Muted(Muted {
            pending: Some(TrackRequest {
                path: path.into(),
                gain: None,
                revision: first(),
            }),
            error: error(),
            config: config(),
            mix: Mix::default(),
        })
    }

    pub(crate) fn live() -> Live {
        Live::new(config())
    }

    pub(crate) fn track_a() -> CurrentTrack {
        CurrentTrack {
            total: Some(TOTAL),
            gain: None,
            path: "/a".into(),
        }
    }

    pub(crate) fn preload_b() -> PreloadedTrack {
        PreloadedTrack {
            path: "/b".into(),
            gain: None,
            total: Some(PRELOAD_TOTAL),
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
            after_load: AfterLoad::None,
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
                after_load: AfterLoad::Resume {
                    position: seconds(5),
                    playback: Playback::Paused,
                    total: Some(TOTAL),
                },
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

    pub(crate) fn handing_over(outgoing: Outgoing, incoming: Incoming) -> Live {
        Live {
            phase: Phase::Handover(Handover { outgoing, incoming }),
            ..playing_with_crossfade()
        }
    }

    pub(crate) fn retiring(from: f32) -> Live {
        handing_over(Outgoing { from }, Incoming::Playing(track_b()))
    }

    pub(crate) fn crossfading(fade: Fade) -> Live {
        Live {
            phase: Phase::Playing(Playing {
                next: Next::Crossfading {
                    preload: preload_b(),
                    fade,
                },
                ..Playing::new(track_a())
            }),
            config: EngineConfig {
                crossfade: crossfade(10),
                ..config()
            },
            ..live()
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
        cmd(AudioCmd::Load {
            path: path.into(),
            gain: None,
            revision,
        })
    }

    pub(crate) fn load(path: &str) -> EngineMessage {
        load_at(path, first())
    }

    pub(crate) fn preload_at(path: &str, revision: Revision) -> EngineMessage {
        cmd(AudioCmd::Preload {
            path: path.into(),
            gain: None,
            revision,
        })
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

    pub(crate) fn installed(preload: PreloadedTrack) -> EngineMessage {
        EngineMessage::Preloaded(Ok(Preload::Crossfade(preload)))
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
}
