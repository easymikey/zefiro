mod closed;
pub(crate) mod crossfade;
pub mod effect;
mod execute;
mod live;
mod machine;
pub mod message;
pub(crate) mod phase;
pub(crate) mod revisions;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use kernel::{
        cmd::{AudioCmd, Cmd, Cmds, Media, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            device::{DeviceName, OutputDevice},
            revision::Revision,
            settings::{AudioSettings, ReplayGain},
            speed::Speed,
            transport::OutputError,
        },
        message::{AudioError, AudioEvent, DecodeError},
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use proptest::test_runner::TestCaseError;

    use crate::{
        AudioDriver,
        deck::{event::DeckEvent, job::AudioJob, source::PreloadMode},
        engine::{
            effect::{AudioLoopCmd, EngineEffect},
            message::{AudioMessage, DeviceOpened, EngineMessage},
            phase::{
                Fade,
                Incoming,
                LoadedTrack,
                Loading,
                NextTrack,
                Phase,
                Playing,
                Resume,
            },
            state::{Closed, Engine, EngineState, ExecutedRevisions, Live},
        },
    };

    #[track_caller]
    pub(crate) fn assert_same<T: std::fmt::Debug>(actual: T, expected: T) {
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    pub(crate) fn executed(
        cmd: Result<AudioLoopCmd, Unhandled>,
    ) -> Result<(Vec<EngineEffect>, Vec<AudioEvent>), Unhandled> {
        cmd.map(|cmd| {
            let (effects, events) = cmd.into_parts();
            let executed = effects
                .into_iter()
                .filter_map(|effect| match effect {
                    LoopEffect::Execute(effect) => Some(effect),
                    LoopEffect::Run(_)
                    | LoopEffect::After { .. }
                    | LoopEffect::Watch { .. }
                    | LoopEffect::Unwatch(_) => None,
                })
                .collect();
            (executed, events)
        })
    }

    pub(crate) const TRACK_A_DURATION: Duration = Duration::from_secs(100);
    pub(crate) const TRACK_B_DURATION: Duration = Duration::from_secs(90);
    pub(crate) const CROSSFADE_SECONDS: u64 = 10;

    pub(crate) fn step(
        state: &mut EngineState,
        message: EngineMessage,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let mut engine = Engine::new(state.clone());
        let cmd = engine.transition(message);
        *state = engine.state;
        cmd
    }

    pub(crate) fn trace(
        state: EngineState,
        messages: Vec<EngineMessage>,
    ) -> Result<(EngineState, Vec<<AudioDriver as Machine>::Effect>), Unhandled> {
        let mut engine = Engine::new(state);
        let log = messages
            .into_iter()
            .map(|message| engine.transition(message))
            .collect::<Result<Vec<_>, Unhandled>>()?;
        Ok((engine.state, log))
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

    pub(crate) fn settings_on(device_name: &str) -> AudioSettings {
        AudioSettings {
            device: OutputDevice::Named(
                DeviceName::new(device_name.to_string()).unwrap(),
            ),
            ..settings()
        }
    }

    pub(crate) fn decode_error() -> AudioError {
        AudioError::Decode {
            path: "/a".into(),
            error: DecodeError::Unsupported,
        }
    }

    pub(crate) fn preload_error() -> AudioError {
        AudioError::Preload {
            path: "/b".into(),
            error: DecodeError::Unsupported,
        }
    }

    pub(crate) fn device_error() -> AudioError {
        AudioError::OpenDevice {
            requested_device: OutputDevice::SystemDefault,
            diagnostic: kernel::domain::config::Diagnostic::from_error(
                &std::io::Error::other("no output device available"),
            ),
        }
    }

    pub(crate) fn failed() -> AudioMessage {
        AudioMessage::Deck(DeckEvent::OutputLost(OutputError::DeviceGone))
    }

    pub(crate) fn driver_with(engine_state: EngineState) -> AudioDriver {
        let (spectrum_buffers, _spectrum_tap) = crate::tap::spectrum_channel();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (feed_sender, feed_receiver) = crossbeam_channel::bounded(4);
        AudioDriver {
            engine: Engine::new(engine_state),
            deck: crate::deck::Deck::new(
                spectrum_buffers,
                callback_sender,
                feed_sender,
            ),
            feed_receiver: Some(feed_receiver),
        }
    }

    pub(crate) fn closed() -> EngineState {
        EngineState::Closed(Closed {
            settings: settings(),
            track_load: None,
            speed: Speed::default(),
        })
    }

    pub(crate) fn waiting_for(path: &str) -> EngineState {
        EngineState::Closed(Closed {
            track_load: Some(TrackLoad {
                media: Media::Local(path.into()),
                decibels: None,
                revision: first(),
            }),
            settings: settings(),
            speed: Speed::default(),
        })
    }

    pub(crate) fn live() -> Live {
        Live::new(settings(), Speed::default())
    }

    pub(crate) fn track_a() -> LoadedTrack {
        LoadedTrack {
            duration: Some(TRACK_A_DURATION),
            decibels: None,
            media: Media::Local("/a".into()),
        }
    }

    pub(crate) fn track_b() -> LoadedTrack {
        LoadedTrack {
            duration: Some(TRACK_B_DURATION),
            decibels: None,
            media: Media::Local("/b".into()),
        }
    }

    pub(crate) fn playing_track(current: LoadedTrack) -> Phase {
        Phase::Playing(Playing::new(current))
    }

    pub(crate) fn loading_track(path: &str) -> Loading {
        Loading {
            media: Media::Local(path.into()),
            decibels: None,
            resume: None,
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
                media: Media::Local("/a".into()),
                decibels: None,
                resume: Some(Resume {
                    position: seconds(5),
                    playback: Playback::Paused,
                    duration: Some(TRACK_A_DURATION),
                    upcoming: None,
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
            phase: Phase::Handover(incoming),
            ..playing_with_crossfade()
        }
    }

    pub(crate) fn handed_over_to_b() -> Live {
        handing_over(Incoming::Playing(track_b()))
    }

    pub(crate) fn crossfading_idle() -> Live {
        Live {
            phase: Phase::Playing(Playing {
                next: NextTrack::Crossfading {
                    incoming: track_b(),
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
                next: NextTrack::Crossfading {
                    incoming: track_b(),
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

    pub(crate) fn cmd(audio_cmd: AudioCmd) -> EngineMessage {
        EngineMessage::Cmds(Cmds {
            cmds: vec![audio_cmd],
            at: Instant::now(),
        })
    }

    pub(crate) fn first() -> Revision {
        Revision::default().next()
    }

    pub(crate) fn second() -> Revision {
        first().next()
    }

    pub(crate) fn load_with_revision(path: &str, revision: Revision) -> EngineMessage {
        cmd(AudioCmd::Load(TrackLoad {
            media: Media::Local(path.into()),
            decibels: None,
            revision,
        }))
    }

    pub(crate) fn load(path: &str) -> EngineMessage {
        load_with_revision(path, first())
    }

    pub(crate) fn preload_with_revision(
        path: &str,
        revision: Revision,
    ) -> EngineMessage {
        cmd(AudioCmd::Preload(TrackLoad {
            media: Media::Local(path.into()),
            decibels: None,
            revision,
        }))
    }

    pub(crate) fn preload(path: &str) -> EngineMessage {
        preload_with_revision(path, first())
    }

    pub(crate) fn with_load_revision(live: Live, revision: Revision) -> Live {
        Live {
            executed_revisions: ExecutedRevisions {
                load: revision,
                ..live.executed_revisions
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
                next: NextTrack::Preloading {
                    media: Media::Local(path.into()),
                    decibels: None,
                },
                ..playing
            }),
            ..live
        }
    }

    pub(crate) fn attached(
        incoming: &LoadedTrack,
        revision: Revision,
    ) -> EngineMessage {
        EngineMessage::Attached {
            revision,
            preload_mode: PreloadMode::Crossfade(Speed::default()),
            duration: incoming.duration,
        }
    }

    pub(crate) fn decoding(path: &str) -> AudioLoopCmd {
        Cmd::effect(LoopEffect::Run(AudioJob::Decode {
            media_path: path.into(),
            download: None,
            revision: second(),
        }))
    }

    pub(crate) fn preloading(path: &str) -> AudioLoopCmd {
        Cmd::effect(LoopEffect::Run(AudioJob::Preload {
            media_path: path.into(),
            download: None,
            revision: first(),
        }))
    }

    pub(crate) fn with_preload_revision(live: Live, revision: Revision) -> Live {
        Live {
            executed_revisions: ExecutedRevisions {
                preload: revision,
                ..live.executed_revisions
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
            device_name: None,
            position,
            playback,
        })
    }

    pub(crate) struct EngineRow {
        pub(crate) next: EngineState,
        pub(crate) effect: Result<AudioLoopCmd, Unhandled>,
    }

    pub(crate) fn assert_fallback(engine_state: EngineState, moved_row: EngineRow) {
        let (state, mut log) = trace(
            engine_state,
            vec![
                EngineMessage::NotFound,
                opened(
                    OutputDevice::SystemDefault,
                    Duration::ZERO,
                    Playback::Playing,
                ),
            ],
        )
        .unwrap();
        let EngineRow {
            next,
            effect: expected,
        } = moved_row;
        assert_eq!(state, next);
        assert_same(Ok(log.pop().unwrap()), expected);
    }

    pub(crate) fn assert_cell(
        engine_state: EngineState,
        message: EngineMessage,
        moved_row: EngineRow,
    ) {
        let mut state = engine_state;
        let effect = step(&mut state, message);
        let EngineRow {
            next,
            effect: expected,
        } = moved_row;
        assert_eq!(state, next);
        assert_same(effect, expected);
    }

    #[test]
    fn an_empty_report_is_refused() {
        let mut state = EngineState::Live(live());
        assert_eq!(
            step(&mut state, EngineMessage::Reported(None)).err(),
            Some(Unhandled)
        );
        assert_eq!(state, EngineState::Live(live()));
    }
}
