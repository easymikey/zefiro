use kernel::{
    cmd::{AudioCmd, Cmd, Playback},
    domain::settings::AudioSettings,
    message::{AudioError, AudioEvent},
    update::machine::{LoopEffect, Unhandled},
};

use crate::{
    deck::job::AudioJob,
    engine::{
        effect::{AudioLoopCmd, EngineEffect},
        machine::batched,
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
            AudioCmd::SetDevice(device) => {
                self.settings.device = device.clone();
                Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                    device,
                    speed: self.speed,
                })))
            }
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
            AudioCmd::Stop if self.track_load.is_none() => Err(Unhandled),
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
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(LoopEffect::Run(AudioJob::ListDevices)))
            }
        }
    }

    fn stays_silent(&mut self, error: AudioError) -> AudioLoopCmd {
        self.track_load = None;
        Cmd::message(AudioEvent::Error(error))
    }

    pub(crate) fn reopened(
        self,
        revisions: &mut JobRevisions,
        device_opened: DeviceOpened,
    ) -> (Live, AudioLoopCmd) {
        let DeviceOpened {
            device,
            position: _position,
            playback: _playback,
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
            .map_or_else(Cmd::none, |track_load| live.load(revisions, track_load));
        (live, cmd)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        cmd::{AudioCmd, Cmd, Playback, TrackLoad},
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
            message::{AudioMessage, EngineMessage},
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
    #[case::closed_retries_a_device(
        closed(),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        EngineRow {
            next: EngineState::Closed(Closed { settings: settings_on("usb"), track_load: None, speed: Speed::default() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            }))),
        }
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
    #[case::closed_ignores_a_decode(closed(), EngineMessage::Decoded(None))]
    #[case::closed_ignores_a_preload_answer(closed(), attached(&track_b(), Revision::default()))]
    fn a_stale_cell_leaves_the_closed_engine_alone(
        #[case] engine_state: EngineState,
        #[case] message: EngineMessage,
    ) {
        let mut state = engine_state.clone();
        assert_eq!(step(&mut state, message).err(), Some(Unhandled));
        assert_eq!(state, engine_state);
    }

    #[rstest]
    #[case::closed_refuses_a_resume(
        closed(),
        cmd(AudioCmd::SetPlayback(Playback::Playing))
    )]
    #[case::closed_pause_is_refused(
        closed(),
        cmd(AudioCmd::SetPlayback(Playback::Paused))
    )]
    #[case::closed_refuses_seek(closed(), cmd(AudioCmd::Seek(seconds(5))))]
    #[case::closed_refuses_preload(closed(), preload("/b"))]
    fn a_refused_cell_leaves_the_state_and_names_the_error(
        #[case] engine_state: EngineState,
        #[case] message: EngineMessage,
    ) {
        let mut state = engine_state.clone();
        assert_eq!(step(&mut state, message).err(), Some(Unhandled));
        assert_eq!(state, engine_state);
    }

    #[test]
    fn closed_ignores_a_second_output_error() {
        let mut driver = driver_with(closed());
        assert_eq!(driver.transition(failed()).err(), Some(Unhandled));
        assert_eq!(driver.engine.state, closed());
    }

    #[test]
    fn closed_stop_without_a_load_is_refused() {
        let mut state = closed();
        assert_eq!(step(&mut state, cmd(AudioCmd::Stop)).err(), Some(Unhandled));
        assert_eq!(state, closed());
    }

    #[rstest]
    #[case::speed(cmd(AudioCmd::SetSpeed(Speed::default())))]
    #[case::crossfade(set_crossfade(0))]
    #[case::replay_gain(cmd(AudioCmd::SetReplayGain(ReplayGain::Off)))]
    fn closed_same_speed_is_refused(#[case] message: EngineMessage) {
        let mut state = closed();
        assert_eq!(step(&mut state, message).err(), Some(Unhandled));
        assert_eq!(state, closed());
    }

    #[rstest]
    #[case::an_output_error_while_live(
        failed(),
        AudioEvent::OutputLost(OutputError::DeviceGone)
    )]
    #[case::a_reopen_error_while_live(
        AudioMessage::Engine(EngineMessage::Error(device_error())),
        AudioEvent::Error(device_error())
    )]
    fn an_error_mutes_the_engine_once(
        #[case] message: AudioMessage,
        #[case] expected: AudioEvent,
    ) {
        let mut driver = driver_with(EngineState::Live(playing()));
        assert_same(
            driver.transition(message),
            Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Silence))
                .then(Cmd::message(expected))),
        );

        assert_eq!(silenced(driver.engine.state), None);
    }

    #[test]
    fn a_load_while_closed_reopens_and_then_plays() {
        let (engine, mut log) = trace(closed(), vec![load("/a")]).unwrap();
        let track_load = silenced(engine.clone());
        assert_eq!(
            track_load.map(|track_load| track_load.path),
            Some("/a".into())
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
    fn a_load_while_closed_on_a_dead_device_reports_again() {
        let (engine, log) = trace(
            closed(),
            vec![load("/a"), EngineMessage::Error(device_error())],
        )
        .unwrap();
        assert_same(
            log,
            vec![
                Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                    device: OutputDevice::SystemDefault,
                    speed: Speed::default(),
                })),
                Cmd::message(AudioEvent::Error(device_error())),
            ],
        );

        assert_eq!(silenced(engine), None);
    }

    #[rstest]
    #[case::closed_tells_the_world_the_device_fell_back(
        EngineState::Closed(Closed { settings: settings_on("usb"), track_load: None, speed: Speed::default() }),
        EngineRow {
            next: EngineState::Live(live()),
            effect: Ok(Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault))),
        }
    )]
    #[case::a_waiting_load_starts_after_the_fallback_is_announced(
        waiting_for("/a"),
        EngineRow {
            next: EngineState::Live(with_load_revision(loading(), first())),
            effect: Ok(Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)).then(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/a")))),
        }
    )]
    fn a_fallback_is_announced_once_the_system_default_opens(
        #[case] engine_state: EngineState,
        #[case] moved_row: EngineRow,
    ) {
        assert_fallback(engine_state, moved_row);
    }

    #[test]
    fn an_open_that_finds_no_device_asks_for_the_system_default() {
        let output_device =
            OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap());
        let (_, log) = trace(
            closed(),
            vec![
                cmd(AudioCmd::SetDevice(output_device.clone())),
                EngineMessage::NotFound,
                opened(
                    OutputDevice::SystemDefault,
                    Duration::ZERO,
                    Playback::Playing,
                ),
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
                Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)),
            ],
        );
    }
}
