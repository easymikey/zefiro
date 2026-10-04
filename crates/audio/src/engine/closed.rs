use kernel::{
    cmd::{AudioCmd, Cmd, Playback},
    domain::settings::AudioSettings,
    message::{AudioError, AudioEvent},
    update::machine::{Machine, Unhandled},
};

use crate::{
    deck::{DeviceOpened, job::AudioJob},
    engine::{
        effect::EngineEffect,
        machine::batched,
        message::EngineMessage,
        state::{Closed, Live, announce},
    },
};

impl Machine for Closed {
    type Message = EngineMessage;
    type Effect = Cmd<EngineEffect, AudioEvent>;

    fn transition(
        &mut self,
        message: EngineMessage,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match message {
            EngineMessage::Cmds(batch) => batched(batch, |cmd| self.command(cmd)),
            EngineMessage::Error(error @ AudioError::Device { .. }) => {
                Ok(self.stays_silent(error))
            }
            EngineMessage::Error(
                AudioError::Decode { .. }
                | AudioError::Preload { .. }
                | AudioError::ListDevices { .. }
                | AudioError::Stream { .. }
                | AudioError::OutputLost(_)
                | AudioError::Seek { .. },
            )
            | EngineMessage::Opened(_)
            | EngineMessage::Reported(_)
            | EngineMessage::DevicesListed(_)
            | EngineMessage::Decoded(_)
            | EngineMessage::Preloaded(_)
            | EngineMessage::Finished(_)
            | EngineMessage::Cued
            | EngineMessage::Ramped(_) => Err(Unhandled),
        }
    }
}

impl Closed {
    fn command(
        &mut self,
        cmd: AudioCmd,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match cmd {
            AudioCmd::SetDevice(device) => {
                self.settings.device = device.clone();
                Ok(Cmd::effect(EngineEffect::Open {
                    device,
                    speed: self.speed,
                }))
            }
            AudioCmd::Load(load) => {
                self.pending = Some(load);
                Ok(Cmd::effect(EngineEffect::Open {
                    device: self.settings.device.clone(),
                    speed: self.speed,
                }))
            }
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(EngineEffect::Run(AudioJob::ListDevices)))
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
                self.pending = None;
                Ok(Cmd::none())
            }
            AudioCmd::Playback(Playback::Paused) => Ok(Cmd::none()),
            AudioCmd::Playback(Playback::Playing)
            | AudioCmd::Seek(_)
            | AudioCmd::Preload(_) => Err(Unhandled),
        }
    }

    fn stays_silent(&mut self, error: AudioError) -> Cmd<EngineEffect, AudioEvent> {
        self.pending = None;
        Cmd::message(AudioEvent::Error(error))
    }

    pub(crate) fn reopened(
        &self,
        reopened: DeviceOpened,
    ) -> (Live, Cmd<EngineEffect, AudioEvent>) {
        let DeviceOpened { device, opened, .. } = reopened;
        let mut live = Live::new(
            AudioSettings {
                device: device.clone(),
                ..self.settings.clone()
            },
            self.speed,
        );
        let cmd = self
            .pending
            .clone()
            .map_or_else(Cmd::none, |pending| live.load(pending));
        (live, announce(opened, device, cmd))
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
            settings::{AudioSettings, ReplayGain},
            speed::Speed,
        },
        message::{AudioError, AudioEvent},
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::engine::{
        effect::EngineEffect,
        message::EngineMessage,
        state::{Closed, Engine, Live},
        tests::{
            EngineRow,
            TOTAL,
            assert_cell,
            closed,
            cmd,
            crossfade,
            device_error,
            failed,
            fell_back,
            first,
            installed,
            live,
            load,
            loaded_at,
            loading,
            opened,
            output_lost,
            playing,
            preload,
            seconds,
            set_crossfade,
            settings,
            settings_on,
            trace,
            track_b,
            waiting_for,
        },
    };

    fn silenced(engine: Engine) -> Option<TrackLoad> {
        match engine {
            Engine::Closed(Closed {
                settings: held,
                pending,
                ..
            }) => {
                assert_eq!(held, settings());
                pending
            }
            Engine::Live(_) => panic!("the engine must be closed"),
        }
    }

    #[rstest]
    #[case::closed_retries_a_device(
        closed(),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        EngineRow {
            next: Engine::Closed(Closed { settings: settings_on("usb"), pending: None, speed: Speed::default() }),
            effect: Cmd::effect(EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            }),
        }
    )]
    #[case::closed_lists_devices(
        closed(),
        cmd(AudioCmd::ListDevices),
        EngineRow { next: closed(), effect: Cmd::effect(EngineEffect::Run(crate::deck::job::AudioJob::ListDevices))}
    )]
    #[case::closed_goes_live_once_opened(
        closed(),
        opened(
            OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()), Duration::ZERO, Playback::Playing),
        EngineRow { next: Engine::Live(Live { settings: settings_on("usb"), ..live() }), effect: Cmd::none()}
    )]
    #[case::closed_adopts_the_device_that_actually_opened(
        Engine::Closed(Closed { settings: settings_on("usb"), pending: None, speed: Speed::default() }),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        EngineRow { next: Engine::Live(live()), effect: Cmd::none()}
    )]
    #[case::closed_tells_the_world_the_device_fell_back(
        Engine::Closed(Closed { settings: settings_on("usb"), pending: None, speed: Speed::default() }),
        fell_back(Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(live()),
            effect: Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)),
        }
    )]
    #[case::a_waiting_load_starts_once_the_stream_is_back(
        waiting_for("/a"),
        opened(OutputDevice::SystemDefault, Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: Cmd::effect(EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() }),
        }
    )]
    #[case::a_waiting_load_starts_after_the_fallback_is_announced(
        waiting_for("/a"),
        fell_back(Duration::ZERO, Playback::Playing),
        EngineRow {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: Cmd::message(AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)).then(Cmd::effect(EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() })),
        }
    )]
    #[case::a_waiting_load_is_dropped_when_the_stream_stays_dead(
        waiting_for("/a"),
        EngineMessage::Error(device_error()),
        EngineRow {
            next: Engine::Closed(Closed { settings: settings(), pending: None, speed: Speed::default() }),
            effect: Cmd::message(AudioEvent::Error(device_error())),
        }
    )]
    #[case::closed_keeps_the_new_error(
        closed(),
        EngineMessage::Error(device_error()),
        EngineRow {
            next: Engine::Closed(Closed { settings: settings(), pending: None, speed: Speed::default() }),
            effect: Cmd::message(AudioEvent::Error(device_error())),
        }
    )]
    #[case::closed_remembers_the_speed(
        closed(),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        EngineRow {
            next: Engine::Closed(Closed {
                settings: settings(),
                pending: None,
                speed: Speed::clamped(1.5),
            }),
            effect: Cmd::none(),
        }
    )]
    #[case::closed_remembers_the_crossfade(
        closed(),
        set_crossfade(4),
        EngineRow {
            next: Engine::Closed(Closed {
                settings: AudioSettings { crossfade: crossfade(4), ..settings() },
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::none(),
        }
    )]
    #[case::closed_remembers_the_replay_gain(
        closed(),
        cmd(AudioCmd::SetReplayGain(ReplayGain::On)),
        EngineRow {
            next: Engine::Closed(Closed {
                settings: AudioSettings { replay_gain: ReplayGain::On, ..settings() },
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::none(),
        }
    )]
    #[case::closed_stop_clears_a_pending_load(
        waiting_for("/a"),
        cmd(AudioCmd::Stop),
        EngineRow {
            next: Engine::Closed(Closed {
                settings: settings(),
                pending: None,
                speed: Speed::default(),
            }),
            effect: Cmd::none(),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: EngineRow,
    ) {
        assert_cell(start, message, moved);
    }

    #[test]
    fn a_pause_leaves_the_closed_engine_alone() {
        let mut state = closed();
        assert_eq!(
            state.transition(cmd(AudioCmd::Playback(Playback::Paused))),
            Ok(Cmd::none())
        );
        assert_eq!(state, closed());
    }

    #[rstest]
    #[case::closed_ignores_a_decode(closed(), EngineMessage::Decoded(None))]
    #[case::closed_ignores_a_preload_answer(closed(), installed(track_b()))]
    #[case::closed_ignores_a_second_stream_error(closed(), failed())]
    fn a_stale_cell_leaves_the_closed_engine_alone(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.transition(message), Err(Unhandled));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::closed_refuses_a_resume(
        closed(),
        cmd(AudioCmd::Playback(Playback::Playing))
    )]
    #[case::closed_refuses_seek(closed(), cmd(AudioCmd::Seek(seconds(5))))]
    #[case::closed_refuses_preload(closed(), preload("/b"))]
    fn a_refused_cell_leaves_the_state_and_names_the_error(
        #[case] start: Engine,
        #[case] message: EngineMessage,
    ) {
        let mut state = start.clone();
        assert_eq!(state.transition(message), Err(Unhandled));
        assert_eq!(state, start);
    }

    #[rstest]
    #[case::a_stream_error_while_live(failed(), output_lost())]
    #[case::a_reopen_error_while_live(
        EngineMessage::Error(device_error()),
        device_error()
    )]
    fn an_error_mutes_the_engine_once(
        #[case] message: EngineMessage,
        #[case] expected: AudioError,
    ) {
        let (engine, log) = trace(Engine::Live(playing()), vec![message]).unwrap();
        assert_eq!(
            log,
            vec![
                Cmd::effect(EngineEffect::Mute)
                    .then(Cmd::message(AudioEvent::Error(expected)))
            ]
        );

        assert_eq!(silenced(engine), None);
    }

    #[test]
    fn a_load_while_closed_reopens_and_then_plays() {
        let (engine, mut log) = trace(closed(), vec![load("/a")]).unwrap();
        let pending = silenced(engine.clone());
        assert_eq!(pending.map(|pending| pending.path), Some("/a".into()));

        let (engine, tail) = trace(
            engine,
            vec![
                opened(OutputDevice::SystemDefault, seconds(0), Playback::Playing),
                EngineMessage::Decoded(Some(TOTAL)),
            ],
        )
        .unwrap();
        log.extend(tail);
        insta::assert_debug_snapshot!(log);
        assert!(matches!(engine, Engine::Live(_)));
    }

    #[test]
    fn a_load_while_closed_on_a_dead_device_reports_again() {
        let (engine, log) = trace(
            closed(),
            vec![load("/a"), EngineMessage::Error(device_error())],
        )
        .unwrap();
        assert_eq!(
            log,
            vec![
                Cmd::effect(EngineEffect::Open {
                    device: OutputDevice::SystemDefault,
                    speed: Speed::default()
                }),
                Cmd::message(AudioEvent::Error(device_error())),
            ]
        );

        assert_eq!(silenced(engine), None);
    }
}
