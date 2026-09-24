use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioEvent,
    AudioFailure,
    domain::{Bounded, Percent, Revision, Speed},
    update::Rejected,
};

use crate::{
    EngineConfig,
    deck::DeviceOpen,
    engine::{
        crossfade::{effective_volume, gain_in, gain_out},
        effect::EngineEffect,
        phase::{Fade, Handover, Next, Phase, Playing},
    },
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Engine {
    Muted(Muted),
    Live(Live),
}

impl Engine {
    pub(crate) const TICK: Duration = Duration::from_millis(100);
}

impl Default for Engine {
    fn default() -> Self {
        Engine::Live(Live::new(EngineConfig::default()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Muted {
    pub(crate) fault: AudioFailure,
    pub(crate) config: EngineConfig,
    pub(crate) pending: Option<PendingLoad>,
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
pub(crate) struct PendingLoad {
    pub(crate) path: PathBuf,
    pub(crate) gain: Option<f32>,
    pub(crate) revision: Revision,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Live {
    pub(crate) phase: Phase,
    pub(crate) speed: Speed,
    pub(crate) user_factor: Percent,
    pub(crate) config: EngineConfig,
    pub(crate) performed: Stamps,
    pub(crate) last_position: Option<Duration>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Stamps {
    pub(crate) load: Revision,
    pub(crate) preload: Revision,
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
            user_factor: mix.volume,
            config,
            performed: Stamps::default(),
            last_position: None,
        }
    }

    pub(crate) fn volume(&self) -> f32 {
        let gain = self.phase.current().and_then(|current| current.gain);
        effective_volume(&self.config, gain, self.user_factor.ratio())
    }

    pub(crate) fn primary_gain(&self) -> f32 {
        let volume = self.volume();
        match &self.phase {
            Phase::Handover(Handover { outgoing, .. }) => {
                volume * gain_in(outgoing.fraction)
            }
            Phase::Playing(Playing {
                next:
                    Next::Crossfading {
                        fade: Fade::Fading(fraction),
                        ..
                    },
                ..
            }) => volume * gain_out(*fraction),
            Phase::Idle
            | Phase::Loading(_)
            | Phase::Playing(Playing {
                next:
                    Next::None
                    | Next::Crossfading {
                        fade: Fade::Idle, ..
                    },
                ..
            }) => volume,
        }
    }
}

pub(crate) fn announce(
    opened: DeviceOpen,
    device: Option<String>,
    io: EngineEffect,
) -> EngineEffect {
    match opened {
        DeviceOpen::AsRequested => io,
        DeviceOpen::FellBack => notice(device, io),
    }
}

fn notice(device: Option<String>, then: EngineEffect) -> EngineEffect {
    let told = EngineEffect::Send(AudioEvent::DeviceFellBack(device));
    if matches!(then, EngineEffect::Nothing) {
        return told;
    }
    EngineEffect::Many(vec![told, then])
}

pub(crate) enum Transition {
    Next(Engine, EngineEffect),
    Rejected(Rejected<Engine>),
}

impl From<(Engine, EngineEffect)> for Transition {
    fn from(moved: (Engine, EngineEffect)) -> Self {
        let (engine, io) = moved;
        Transition::Next(engine, io)
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::time::Duration;

    use kernel::{
        AudioCmd,
        AudioFailure,
        Bounded,
        Playback,
        domain::{Crossfade, Replaygain, Revision},
    };

    use crate::{
        EngineConfig,
        UnityVolume,
        deck::{DeviceOpen, Reopening},
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
            state::{Engine, Live, Mix, Muted, PendingLoad, Stamps},
        },
    };

    pub(crate) const TOTAL: Duration = Duration::from_secs(100);
    pub(crate) const PRELOAD_TOTAL: Duration = Duration::from_secs(90);
    pub(crate) const CROSSFADE_SECONDS: u64 = 10;

    pub(crate) fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    pub(crate) fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(secs(seconds))
    }

    pub(crate) fn config() -> EngineConfig {
        EngineConfig {
            crossfade: crossfade(0),
            replaygain: Replaygain::Off,
            unity_volume: UnityVolume::Free,
            device: None,
        }
    }

    pub(crate) fn config_on(device: &str) -> EngineConfig {
        EngineConfig {
            device: Some(device.to_string()),
            ..config()
        }
    }

    pub(crate) fn fault() -> AudioFailure {
        AudioFailure::Stream {
            reason: "no output device available".to_string(),
        }
    }

    pub(crate) fn decode_fault() -> AudioFailure {
        AudioFailure::Decode {
            path: "/a".into(),
            reason: "not a flac".to_string(),
        }
    }

    pub(crate) fn output_lost() -> AudioFailure {
        AudioFailure::OutputLost {
            reason: "the device went away".to_string(),
        }
    }

    pub(crate) fn failed() -> EngineMessage {
        EngineMessage::Failed(output_lost())
    }

    pub(crate) fn muted() -> Engine {
        Engine::Muted(Muted {
            fault: fault(),
            config: config(),
            pending: None,
            mix: Mix::default(),
        })
    }

    pub(crate) fn waiting_for(path: &str) -> Engine {
        Engine::Muted(Muted {
            pending: Some(PendingLoad {
                path: path.into(),
                gain: None,
                revision: first(),
            }),
            fault: fault(),
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
        Phase::Playing(Playing {
            previous_queue_len: 1,
            ..Playing::new(current)
        })
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
                    position: secs(5),
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

    pub(crate) fn retiring(fraction: f32) -> Live {
        handing_over(
            Outgoing {
                from: 1.0,
                fraction,
            },
            Incoming::Playing(track_b()),
        )
    }

    pub(crate) fn crossfading(fade: Fade) -> Live {
        Live {
            phase: Phase::Playing(Playing {
                next: Next::Crossfading {
                    preload: preload_b(),
                    fade,
                },
                previous_queue_len: 1,
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

    pub(crate) fn third() -> Revision {
        second().next()
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
            performed: Stamps {
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

    pub(crate) fn landed(preload: PreloadedTrack) -> EngineMessage {
        EngineMessage::Preloaded(Ok(Preload::Crossfade(preload)))
    }

    pub(crate) fn preloaded_at(live: Live, revision: Revision) -> Live {
        Live {
            performed: Stamps {
                preload: revision,
                ..live.performed
            },
            ..live
        }
    }

    pub(crate) fn set_crossfade(seconds: u64) -> EngineMessage {
        cmd(AudioCmd::SetCrossfade(crossfade(seconds)))
    }

    pub(crate) fn opened(
        device: Option<&str>,
        position: Duration,
        playback: Playback,
    ) -> EngineMessage {
        EngineMessage::Opened(Ok(Reopening {
            device: device.map(str::to_string),
            position,
            playback,
            opened: DeviceOpen::AsRequested,
        }))
    }

    pub(crate) fn fell_back(position: Duration, playback: Playback) -> EngineMessage {
        EngineMessage::Opened(Ok(Reopening {
            device: None,
            position,
            playback,
            opened: DeviceOpen::FellBack,
        }))
    }

    pub(crate) fn observed(queue_len: usize, position: Duration) -> EngineMessage {
        EngineMessage::Observed {
            queue_len,
            position,
        }
    }
}
