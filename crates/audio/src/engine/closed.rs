use std::{
    collections::{HashSet, VecDeque},
    mem::discriminant,
};

use kernel::{
    cmd::{AudioCmd, Cmd, Cmds, Media, Playback, TrackLoad},
    domain::{revision::Revision, settings::AudioSettings},
    message::{AudioError, AudioEvent},
    update::machine::{LoopEffect, Unhandled, each_handled},
};

use crate::{
    deck::job::AudioJob,
    engine::{
        effect::{AudioLoopCmd, EngineEffect},
        message::{ClosedMessage, DeviceOpened},
        revisions::JobRevisions,
        state::{Closed, Live},
    },
};

impl Closed {
    pub(crate) fn transition(
        &mut self,
        message: ClosedMessage,
    ) -> Result<AudioLoopCmd, Unhandled> {
        match message {
            ClosedMessage::Cmds(batch) => batched(batch, |cmd| self.command(cmd)),
            ClosedMessage::Error(error) => Ok(self.stays_silent(error)),
        }
    }

    fn command(&mut self, audio_cmd: AudioCmd) -> Result<AudioLoopCmd, Unhandled> {
        match audio_cmd {
            AudioCmd::Load(track_load) => {
                self.track_load = Some(track_load);
                Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                    device: self.settings.device.clone(),
                    speed: self.speed,
                })))
            }
            AudioCmd::SetSpeed(speed) if speed == self.speed => Err(Unhandled),
            AudioCmd::SetCrossfade(crossfade)
                if crossfade == self.settings.crossfade =>
            {
                Err(Unhandled)
            }
            AudioCmd::SetReplayGain(replay_gain)
                if replay_gain == self.settings.replay_gain =>
            {
                Err(Unhandled)
            }
            AudioCmd::SetDevice(device) if device == self.settings.device => {
                Err(Unhandled)
            }
            AudioCmd::Stop if self.track_load.is_none() => Err(Unhandled),
            AudioCmd::SetDevice(device) => {
                self.settings.device = device;
                Ok(Cmd::none())
            }
            AudioCmd::SetSpeed(speed) => {
                self.speed = speed;
                Ok(Cmd::none())
            }
            AudioCmd::SetCrossfade(crossfade) => {
                self.settings.crossfade = crossfade;
                Ok(Cmd::none())
            }
            AudioCmd::SetReplayGain(replay_gain) => {
                self.settings.replay_gain = replay_gain;
                Ok(Cmd::none())
            }
            AudioCmd::Stop => {
                self.track_load = None;
                Ok(Cmd::none())
            }
            AudioCmd::SetPlayback(Playback::Paused | Playback::Playing)
            | AudioCmd::Seek(_)
            | AudioCmd::Preload(_) => Err(Unhandled),
            AudioCmd::CancelPreload(revision) => {
                Ok(Cmd::message(AudioEvent::PreloadCancelled(revision)))
            }
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(LoopEffect::Run(AudioJob::ListDevices)))
            }
            AudioCmd::Grow {
                revision,
                downloaded,
            } => self.grow(revision, downloaded),
        }
    }

    fn grow(
        &mut self,
        revision: Revision,
        downloaded: u64,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let Some(TrackLoad {
            media: Media::Growing(growing_media),
            ..
        }) = &mut self.track_load
        else {
            return Err(Unhandled);
        };
        if growing_media.revision != revision {
            return Err(Unhandled);
        }
        growing_media.downloaded = growing_media.downloaded.max(downloaded);
        Ok(Cmd::none())
    }

    fn stays_silent(&mut self, error: AudioError) -> AudioLoopCmd {
        self.track_load = None;
        Cmd::message(AudioEvent::Error(error))
    }

    pub(crate) fn reopened(
        self,
        job_revisions: &mut JobRevisions,
        device_opened: DeviceOpened,
    ) -> (Live, AudioLoopCmd) {
        let DeviceOpened {
            device,
            device_name: _,
            position: _,
            playback: _,
        } = device_opened;
        let mut live = Live::new(
            AudioSettings {
                device,
                ..self.settings
            },
            self.speed,
        );
        let cmd = self
            .track_load
            .map_or_else(Cmd::none, |track_load| live.load(job_revisions, track_load));
        (live, cmd)
    }
}

pub(crate) fn keep_last_idempotent(audio_cmds: Vec<AudioCmd>) -> Vec<AudioCmd> {
    let (_seen, kept) = audio_cmds.into_iter().rev().fold(
        (HashSet::new(), VecDeque::new()),
        |(mut seen, mut kept), cmd| {
            let fresh = match &cmd {
                AudioCmd::Load(_) | AudioCmd::Stop => {
                    seen.clear();
                    true
                }
                AudioCmd::SetSpeed(_) | AudioCmd::Seek(_) => {
                    seen.insert(discriminant(&cmd))
                }
                AudioCmd::SetPlayback(_)
                | AudioCmd::Preload(_)
                | AudioCmd::CancelPreload(_)
                | AudioCmd::SetCrossfade(_)
                | AudioCmd::SetReplayGain(_)
                | AudioCmd::SetDevice(_)
                | AudioCmd::ListDevices
                | AudioCmd::Grow { .. } => true,
            };
            if fresh {
                kept.push_front(cmd);
            }
            (seen, kept)
        },
    );
    Vec::from(kept)
}

pub(crate) fn batched(
    cmds: Cmds<AudioCmd>,
    run: impl FnMut(AudioCmd) -> Result<AudioLoopCmd, Unhandled>,
) -> Result<AudioLoopCmd, Unhandled> {
    each_handled(keep_last_idempotent(cmds.cmds), run)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    use kernel::{
        cmd::{AudioCmd, Cmd, Cmds, GrowingMedia, Media, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            device::{DeviceName, OutputDevice},
            revision::Revision,
            settings::{AudioSettings, ReplayGain},
            speed::Speed,
            transport::OutputError,
        },
        message::AudioEvent,
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        deck::job::AudioJob,
        engine::{
            effect::EngineEffect,
            message::{DeviceOpened, EngineMessage},
            state::{Closed, EngineState, Live},
            tests::{
                EngineRow,
                TRACK_A_DURATION,
                assert_cell,
                assert_fallback,
                assert_same,
                attached,
                closed,
                cmd,
                crossfade,
                decoding,
                device_error,
                driver_with,
                failed,
                first,
                live,
                load,
                loading,
                opened,
                playing,
                preload,
                second,
                seconds,
                set_crossfade,
                settings,
                settings_on,
                step,
                trace,
                track_b,
                waiting_for,
                with_load_revision,
            },
        },
    };

    fn silenced(engine_state: EngineState) -> Option<TrackLoad> {
        match engine_state {
            EngineState::Closed(Closed {
                settings: held,
                track_load,
                ..
            }) => {
                assert_eq!(held, settings());
                track_load
            }
            EngineState::Live(_) => panic!("the engine must be closed"),
        }
    }

    #[rstest]
    #[case::closed_records_a_device(
        closed(),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        EngineRow {
            next: EngineState::Closed(Closed { settings: settings_on("usb"), track_load: None, speed: Speed::default() }),
            effect: Ok(Cmd::none()),
        }
    )]
    #[case::closed_answers_a_cancel_preload_with_cancelled(
        closed(),
        cmd(AudioCmd::CancelPreload(first())),
        EngineRow { next: closed(), effect: Ok(Cmd::message(AudioEvent::PreloadCancelled(first()))) }
    )]
    #[case::closed_lists_devices(
        closed(),
        cmd(AudioCmd::ListDevices),
        EngineRow { next: closed(), effect: Ok(Cmd::effect(LoopEffect::Run(AudioJob::ListDevices)))}
    )]
    #[case::closed_goes_live_once_opened(
        closed(),
        opened(
            OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()), Duration::ZERO, Playback::Playing),
        EngineRow { next: EngineState::Live(Live { settings: settings_on("usb"), ..live() }), effect: Ok(Cmd::none())}
    )]
    #[case::closed_adopts_the_device_that_actually_opened(
        EngineState::Closed(Closed { settings: settings_on("usb"), track_load: None, speed: Speed::default() }),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::none())}
    )]
    #[case::a_waiting_load_starts_once_the_stream_is_back(
        waiting_for("/a"),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: EngineState::Live(with_load_revision(loading(), first())),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/a"))),
        }
    )]
    #[case::a_waiting_load_is_dropped_when_the_stream_stays_dead(
        waiting_for("/a"),
        EngineMessage::Error(device_error()),
        EngineRow {
            next: EngineState::Closed(Closed { settings: settings(), track_load: None, speed: Speed::default() }),
            effect: Ok(Cmd::message(AudioEvent::Error(device_error()))),
        }
    )]
    #[case::closed_keeps_the_new_error(
        closed(),
        EngineMessage::Error(device_error()),
        EngineRow {
            next: EngineState::Closed(Closed { settings: settings(), track_load: None, speed: Speed::default() }),
            effect: Ok(Cmd::message(AudioEvent::Error(device_error()))),
        }
    )]
    #[case::closed_remembers_the_speed(
        closed(),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        EngineRow {
            next: EngineState::Closed(Closed {
                settings: settings(),
                track_load: None,
                speed: Speed::clamped(1.5),
            }),
            effect: Ok(Cmd::none()),
        }
    )]
    #[case::closed_remembers_the_crossfade(
        closed(),
        set_crossfade(4),
        EngineRow {
            next: EngineState::Closed(Closed {
                settings: AudioSettings { crossfade: crossfade(4), ..settings() },
                track_load: None,
                speed: Speed::default(),
            }),
            effect: Ok(Cmd::none()),
        }
    )]
    #[case::closed_remembers_the_replay_gain(
        closed(),
        cmd(AudioCmd::SetReplayGain(ReplayGain::On)),
        EngineRow {
            next: EngineState::Closed(Closed {
                settings: AudioSettings { replay_gain: ReplayGain::On, ..settings() },
                track_load: None,
                speed: Speed::default(),
            }),
            effect: Ok(Cmd::none()),
        }
    )]
    #[case::closed_stop_clears_a_pending_load(
        waiting_for("/a"),
        cmd(AudioCmd::Stop),
        EngineRow {
            next: EngineState::Closed(Closed {
                settings: settings(),
                track_load: None,
                speed: Speed::default(),
            }),
            effect: Ok(Cmd::none()),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] engine_state: EngineState,
        #[case] message: EngineMessage,
        #[case] moved_row: EngineRow,
    ) {
        assert_cell(engine_state, message, moved_row);
    }

    #[rstest]
    #[case::closed_ignores_a_decode(EngineMessage::Decoded(None))]
    #[case::closed_ignores_a_preload_answer(attached(&track_b(), Revision::default()))]
    #[case::closed_refuses_a_resume(cmd(AudioCmd::SetPlayback(Playback::Playing)))]
    #[case::closed_pause_is_refused(cmd(AudioCmd::SetPlayback(Playback::Paused)))]
    #[case::closed_refuses_seek(cmd(AudioCmd::Seek(seconds(5))))]
    #[case::closed_refuses_preload(preload("/b"))]
    #[case::closed_stop_without_a_load_is_refused(cmd(AudioCmd::Stop))]
    #[case::same_speed(cmd(AudioCmd::SetSpeed(Speed::default())))]
    #[case::same_crossfade(set_crossfade(0))]
    #[case::same_replay_gain(cmd(AudioCmd::SetReplayGain(ReplayGain::Off)))]
    #[case::same_device(cmd(AudioCmd::SetDevice(OutputDevice::SystemDefault)))]
    fn a_refused_cell_leaves_the_closed_engine_alone(#[case] message: EngineMessage) {
        let mut state = closed();
        assert_eq!(step(&mut state, message).err(), Some(Unhandled));
        assert_eq!(state, closed());
    }

    #[test]
    fn closed_ignores_a_second_output_error() {
        let mut driver = driver_with(closed());
        assert_eq!(driver.transition(failed()).err(), Some(Unhandled));
        assert_eq!(driver.engine.state, closed());
    }

    #[rstest]
    #[case::playing(playing())]
    #[case::the_closed_engine_keeps_the_settings_and_the_speed(Live {
        settings: AudioSettings { crossfade: crossfade(4), ..settings_on("usb") },
        speed: Speed::clamped(1.5),
        ..playing()
    })]
    fn an_output_error_mutes_the_live_engine(#[case] live: Live) {
        let mut driver = driver_with(EngineState::Live(live.clone()));
        assert_same(
            driver.transition(failed()),
            Ok(
                Cmd::effect(LoopEffect::Execute(EngineEffect::Silence)).then(
                    Cmd::message(AudioEvent::OutputLost(OutputError::DeviceGone)),
                ),
            ),
        );

        assert_eq!(
            driver.engine.state,
            EngineState::Closed(Closed {
                settings: live.settings,
                track_load: None,
                speed: live.speed,
            })
        );
    }

    #[test]
    fn a_load_while_closed_reopens_and_then_plays() {
        let (engine, mut log) = trace(closed(), vec![load("/a")]).unwrap();
        let track_load = silenced(engine.clone());
        assert_eq!(
            track_load.map(|track_load| track_load.media),
            Some(Media::Local("/a".into()))
        );

        let (engine, tail) = trace(
            engine,
            vec![
                opened(OutputDevice::SystemDefault, seconds(0), Playback::Playing),
                EngineMessage::Decoded(Some(TRACK_A_DURATION)),
            ],
        )
        .unwrap();
        log.extend(tail);
        insta::assert_debug_snapshot!(log);
        assert!(matches!(engine, EngineState::Live(_)));
    }

    #[test]
    fn a_fallback_is_announced_once_the_system_default_opens() {
        assert_fallback(
            EngineState::Closed(Closed {
                settings: settings_on("usb"),
                track_load: None,
                speed: Speed::default(),
            }),
            EngineRow {
                next: EngineState::Live(live()),
                effect: Ok(Cmd::message(AudioEvent::DeviceFellBack(
                    OutputDevice::SystemDefault,
                ))),
            },
        );
    }

    fn device_then_load(output_device: OutputDevice) -> EngineMessage {
        EngineMessage::Cmds(Cmds {
            cmds: vec![
                AudioCmd::SetDevice(output_device),
                AudioCmd::Load(TrackLoad {
                    media: Media::Local("/a".into()),
                    decibels: None,
                    revision: first(),
                }),
            ],
            at: Instant::now(),
        })
    }

    fn speakers() -> DeviceName {
        DeviceName::new("Speakers".to_string()).unwrap()
    }

    fn opened_on_speakers() -> EngineMessage {
        EngineMessage::Opened(DeviceOpened {
            device: OutputDevice::SystemDefault,
            device_name: Some(speakers()),
            position: Duration::ZERO,
            playback: Playback::Playing,
        })
    }

    #[test]
    fn a_fallback_names_the_opened_device_after_the_fallback() {
        let output_device =
            OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap());
        let (_, log) = trace(
            closed(),
            vec![
                device_then_load(output_device.clone()),
                EngineMessage::NotFound,
                opened_on_speakers(),
            ],
        )
        .unwrap();
        assert_same(
            log,
            vec![
                Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                    device: output_device,
                    speed: Speed::default(),
                })),
                Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                    device: OutputDevice::SystemDefault,
                    speed: Speed::default(),
                })),
                Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault))
                    .then(Cmd::message(AudioEvent::DeviceOpened(speakers())))
                    .then(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(
                        Speed::default(),
                    ))))
                    .then(decoding("/a")),
            ],
        );
    }

    fn growing_load(downloaded: u64) -> TrackLoad {
        TrackLoad {
            media: Media::Growing(GrowingMedia {
                media_path: "/a".into(),
                downloaded,
                byte_len: 1_000,
                revision: second().next(),
            }),
            decibels: None,
            revision: first(),
        }
    }

    #[test]
    fn a_grow_while_closed_raises_the_held_download_and_another_is_refused() {
        let mut engine_state = EngineState::Closed(Closed {
            settings: settings(),
            track_load: Some(growing_load(100)),
            speed: Speed::default(),
        });

        let grown = step(
            &mut engine_state,
            cmd(AudioCmd::Grow {
                revision: second().next(),
                downloaded: 600,
            }),
        );
        let other = step(
            &mut engine_state,
            cmd(AudioCmd::Grow {
                revision: first(),
                downloaded: 900,
            }),
        );

        assert_eq!(grown.map(|cmd| cmd.effects().count()), Ok(0));
        assert_eq!(other.err(), Some(Unhandled));
        assert_eq!(silenced(engine_state), Some(growing_load(600)));
    }

    fn local_load(path: &str) -> AudioCmd {
        AudioCmd::Load(TrackLoad {
            media: Media::Local(PathBuf::from(path)),
            decibels: None,
            revision: Revision::default(),
        })
    }

    #[rstest]
    #[case::two_speeds(
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ],
        vec![AudioCmd::SetSpeed(Speed::clamped(2.0))]
    )]
    #[case::speed_seek_speed(
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::Seek(Duration::from_secs(1)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ],
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ]
    )]
    #[case::seeks_across_a_load(
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            local_load("/b"),
            AudioCmd::Seek(Duration::from_secs(2)),
            AudioCmd::Seek(Duration::from_secs(3)),
        ],
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            local_load("/b"),
            AudioCmd::Seek(Duration::from_secs(3)),
        ]
    )]
    #[case::stop_splits(
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::Stop,
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ],
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::Stop,
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ]
    )]
    #[case::others_untouched(
        vec![
            AudioCmd::SetPlayback(Playback::Playing),
            AudioCmd::SetPlayback(Playback::Playing),
            AudioCmd::ListDevices,
        ],
        vec![
            AudioCmd::SetPlayback(Playback::Playing),
            AudioCmd::SetPlayback(Playback::Playing),
            AudioCmd::ListDevices,
        ]
    )]
    #[case::empty(Vec::new(), Vec::new())]
    fn coalesced_keeps_the_last_idempotent_command(
        #[case] audio_cmds: Vec<AudioCmd>,
        #[case] expected: Vec<AudioCmd>,
    ) {
        assert_eq!(
            crate::engine::closed::keep_last_idempotent(audio_cmds),
            expected
        );
    }
}
