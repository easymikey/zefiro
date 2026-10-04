mod closed;
pub(crate) mod crossfade;
pub mod effect;
mod execute;
mod live;
mod machine;
pub(crate) mod message;
pub(crate) mod phase;
pub(crate) mod revisions;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use kernel::{
        cmd::{AudioCmd, Cmd, Cmds, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            device::{DeviceName, OutputDevice},
            revision::Revision,
            settings::{AudioSettings, ReplayGain},
            speed::Speed,
            transport::StreamError,
        },
        message::{AudioError, AudioEvent, DecodeError},
        update::machine::{Machine, Unhandled},
    };
    use proptest::test_runner::TestCaseError;

    use crate::{
        deck::source::PreloadMode,
        engine::{
            effect::EngineEffect,
            message::{DeviceChoice, DeviceOpened, EngineMessage},
            phase::{
                CurrentTrack,
                Fade,
                Handover,
                Incoming,
                Loading,
                Next,
                Phase,
                Playing,
                Resume,
            },
            state::{Closed, Engine, ExecutedRevisions, Live},
        },
    };

    pub(crate) const TOTAL: Duration = Duration::from_secs(100);
    pub(crate) const PRELOAD_TOTAL: Duration = Duration::from_secs(90);
    pub(crate) const CROSSFADE_SECONDS: u64 = 10;

    pub(crate) fn trace(
        state: Engine,
        messages: Vec<EngineMessage>,
    ) -> Result<(Engine, Vec<Cmd<EngineEffect, AudioEvent>>), Unhandled> {
        let mut current = state;
        let log = messages
            .into_iter()
            .map(|message| current.transition(message))
            .collect::<Result<Vec<_>, Unhandled>>()?;
        Ok((current, log))
    }

    pub(crate) fn unhandled(reason: impl std::fmt::Debug) -> TestCaseError {
        TestCaseError::fail(format!(
            "the machine refused a valid transition: {reason:?}"
        ))
    }

    pub(crate) fn seconds(count: u64) -> Duration {
        Duration::from_secs(count)
    }

    pub(crate) fn crossfade(count: u64) -> Crossfade {
        Crossfade::clamped(seconds(count))
    }

    pub(crate) fn settings() -> AudioSettings {
        AudioSettings {
            crossfade: crossfade(0),
            replay_gain: ReplayGain::Off,
            device: OutputDevice::SystemDefault,
            ..AudioSettings::default()
        }
    }

    pub(crate) fn settings_on(device: &str) -> AudioSettings {
        AudioSettings {
            device: OutputDevice::Named(DeviceName::new(device.to_string()).unwrap()),
            ..settings()
        }
    }

    pub(crate) fn error() -> AudioError {
        AudioError::Stream {
            reason: kernel::domain::config::Diagnostic::from_error(
                &std::io::Error::other("no output device available"),
            ),
        }
    }

    pub(crate) fn decode_error() -> AudioError {
        AudioError::Decode {
            path: "/a".into(),
            kind: DecodeError::Unsupported,
        }
    }

    pub(crate) fn preload_error() -> AudioError {
        AudioError::Preload {
            path: "/b".into(),
            kind: DecodeError::Unsupported,
        }
    }

    pub(crate) fn device_error() -> AudioError {
        AudioError::Device {
            requested: OutputDevice::SystemDefault,
        }
    }

    pub(crate) fn output_lost() -> AudioError {
        AudioError::OutputLost(StreamError::DeviceGone)
    }

    pub(crate) fn failed() -> EngineMessage {
        EngineMessage::Error(output_lost())
    }

    pub(crate) fn closed() -> Engine {
        Engine::Closed(Closed {
            settings: settings(),
            pending: None,
            speed: Speed::default(),
        })
    }

    pub(crate) fn waiting_for(path: &str) -> Engine {
        Engine::Closed(Closed {
            pending: Some(TrackLoad {
                path: path.into(),
                gain: None,
                revision: first(),
            }),
            settings: settings(),
            speed: Speed::default(),
        })
    }

    pub(crate) fn live() -> Live {
        Live::new(settings(), Speed::default())
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
            settings: settings_on("usb"),
            ..live()
        }
    }

    pub(crate) fn live_with_crossfade(seconds: u64) -> Live {
        Live {
            settings: AudioSettings {
                crossfade: crossfade(seconds),
                ..settings()
            },
            ..live()
        }
    }

    pub(crate) fn playing_with_crossfade() -> Live {
        Live {
            settings: AudioSettings {
                crossfade: crossfade(CROSSFADE_SECONDS),
                ..settings()
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
                    fade: Fade::Armed,
                },
                ..Playing::new(track_a())
            }),
            settings: AudioSettings {
                crossfade: crossfade(CROSSFADE_SECONDS),
                ..settings()
            },
            ..live()
        }
    }

    pub(crate) fn crossfading_mid_ramp() -> Live {
        Live {
            phase: Phase::Playing(Playing {
                next: Next::Crossfading {
                    preload: track_b(),
                    fade: Fade::Running,
                },
                ..Playing::new(track_a())
            }),
            ..crossfading_idle()
        }
    }

    pub(crate) fn promoted(crossfade: Crossfade) -> Live {
        Live {
            phase: playing_track(track_b()),
            settings: AudioSettings {
                crossfade,
                ..settings()
            },
            ..live()
        }
    }

    pub(crate) fn cmd(cmd: AudioCmd) -> EngineMessage {
        EngineMessage::Cmds(Cmds {
            cmds: vec![cmd],
            at: Instant::now(),
        })
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
            executed: ExecutedRevisions {
                load: revision,
                ..live.executed
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
                next: Next::Preloading {
                    path: path.into(),
                    gain: None,
                },
                ..playing
            }),
            ..live
        }
    }

    pub(crate) fn installed(preload: CurrentTrack) -> EngineMessage {
        EngineMessage::Preloaded(PreloadMode::Crossfade {
            track: preload,
            speed: Speed::default(),
        })
    }

    pub(crate) fn gapless_preload(path: &str) -> Cmd<EngineEffect, AudioEvent> {
        Cmd::effect(EngineEffect::Preload(PreloadMode::Gapless(path.into())))
    }

    pub(crate) fn crossfade_preload(path: &str) -> Cmd<EngineEffect, AudioEvent> {
        Cmd::effect(EngineEffect::Preload(PreloadMode::Crossfade {
            track: CurrentTrack {
                total: None,
                gain: None,
                path: path.into(),
            },
            speed: Speed::default(),
        }))
    }

    pub(crate) fn preloaded_at(live: Live, revision: Revision) -> Live {
        Live {
            executed: ExecutedRevisions {
                incoming: revision,
                ..live.executed
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
        EngineMessage::Opened(DeviceOpened {
            device,
            position,
            playback,
            opened: DeviceChoice::Requested,
        })
    }

    pub(crate) fn fell_back(position: Duration, playback: Playback) -> EngineMessage {
        EngineMessage::Opened(DeviceOpened {
            device: OutputDevice::SystemDefault,
            position,
            playback,
            opened: DeviceChoice::FellBack,
        })
    }

    pub(crate) struct EngineRow {
        pub(crate) next: Engine,
        pub(crate) effect: Result<Cmd<EngineEffect, AudioEvent>, Unhandled>,
    }

    pub(crate) fn assert_cell(start: Engine, message: EngineMessage, moved: EngineRow) {
        let mut state = start;
        let effect = state.transition(message);
        let EngineRow {
            next,
            effect: expected,
        } = moved;
        assert_eq!(state, next);
        assert_eq!(effect, expected);
    }
}
