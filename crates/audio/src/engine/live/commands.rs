use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioEvent,
    Playback,
    TrackRequest,
    domain::{Crossfade, ListedDevice, OutputDevice},
    update::Rejected,
};

use crate::{
    deck::source::PreloadRequest,
    engine::{
        crossfade::arm_cue,
        effect::EngineEffect,
        machine::EngineError,
        phase::{CurrentTrack, Handover, Incoming, Loading, Next, Phase, Playing},
        state::{Engine, Live, then_report},
    },
};

impl Live {
    pub(crate) fn command(
        mut self,
        cmd: AudioCmd,
    ) -> Result<(Engine, EngineEffect), Box<Rejected<Engine>>> {
        match cmd {
            AudioCmd::Load(request) => Ok(self.load(request)),
            AudioCmd::Preload(request) => self.preload(request),
            AudioCmd::Playback(playback) => {
                let effect = match playback {
                    Playback::Paused => EngineEffect::Pause,
                    Playback::Playing => EngineEffect::Play,
                };
                Ok((Engine::Live(self), then_report(effect)))
            }
            AudioCmd::Seek(target) => Ok(self.seek(target)),
            AudioCmd::SetSpeed(speed) => {
                self.speed = speed;
                Ok((
                    Engine::Live(self),
                    then_report(EngineEffect::SetSpeed(speed)),
                ))
            }
            AudioCmd::Stop => {
                self.phase = Phase::Idle;
                Ok((Engine::Live(self), EngineEffect::Clear))
            }
            AudioCmd::SetCrossfade(crossfade) => Ok(self.set_crossfade(crossfade)),
            AudioCmd::SetReplaygain(replaygain) => {
                self.config.replaygain = replaygain;
                let volume = self.volume();
                Ok((Engine::Live(self), EngineEffect::SetVolume(volume)))
            }
            AudioCmd::SetDevice(device) => Ok(self.set_device(device)),
            AudioCmd::ListDevices => {
                Ok((Engine::Live(self), EngineEffect::ListDevices))
            }
        }
    }

    pub(crate) fn load(mut self, pending: TrackRequest) -> (Engine, EngineEffect) {
        let TrackRequest {
            path,
            gain,
            revision,
        } = pending;
        if revision.is_stale(self.performed.load) {
            return (Engine::Live(self), EngineEffect::Nothing);
        }
        self.performed.load = revision;
        let speed = self.speed;
        let fades =
            !self.config.crossfade.get().is_zero() && self.phase.current().is_some();
        let loading = Loading {
            path: path.clone(),
            gain,
            after_load: None,
        };
        if fades {
            self.phase = Phase::Handover(Handover {
                incoming: Incoming::Loading(loading),
            });
            return (
                Engine::Live(self),
                EngineEffect::StartHandover { path, speed },
            );
        }
        self.phase = Phase::Loading(loading);
        (Engine::Live(self), EngineEffect::StartLoad { path, speed })
    }

    fn seek(mut self, target: Duration) -> (Engine, EngineEffect) {
        let Some(playing) = self.take_playing() else {
            return (Engine::Live(self), then_report(EngineEffect::Seek(target)));
        };
        let Next::Crossfading { preload, fading } = playing.next else {
            self.phase = Phase::Playing(playing);
            return (Engine::Live(self), then_report(EngineEffect::Seek(target)));
        };
        let cue = arm_cue(playing.current.total, self.config.crossfade.get());
        let rearm = !fading || cue.is_some_and(|cue| target < cue);
        let steps = [
            (fading && rearm).then_some(EngineEffect::CancelCrossfade),
            Some(EngineEffect::Seek(target)),
            rearm.then_some(EngineEffect::Arm { cue }),
            Some(EngineEffect::Report),
        ]
        .into_iter()
        .flatten()
        .collect();
        self.phase = Phase::Playing(Playing {
            next: Next::Crossfading {
                preload,
                fading: fading && !rearm,
            },
            ..playing
        });
        (Engine::Live(self), EngineEffect::Batch(steps))
    }

    fn set_device(self, device: OutputDevice) -> (Engine, EngineEffect) {
        match device {
            in_use if in_use == self.config.device => {
                (Engine::Live(self), EngineEffect::Nothing)
            }
            other => {
                let speed = self.speed;
                (
                    Engine::Live(self),
                    EngineEffect::Open {
                        device: other,
                        speed,
                    },
                )
            }
        }
    }

    fn preload(
        mut self,
        requested: TrackRequest,
    ) -> Result<(Engine, EngineEffect), Box<Rejected<Engine>>> {
        let TrackRequest {
            path,
            gain,
            revision,
        } = requested;
        if revision.is_stale(self.performed.incoming) {
            return Ok((Engine::Live(self), EngineEffect::Nothing));
        }
        let Some(playing) = self.take_playing() else {
            return Err(Box::new(Rejected {
                state: Engine::Live(self),
                reason: EngineError::WhileNotPlaying(path),
            }));
        };
        self.performed.incoming = revision;
        self.phase = Phase::Playing(Playing {
            preloading: Some(path.clone()),
            ..playing
        });
        let request = if self.config.crossfade.get().is_zero() {
            PreloadRequest::Gapless(path)
        } else {
            PreloadRequest::Crossfade {
                path,
                gain,
                speed: self.speed,
            }
        };
        Ok((Engine::Live(self), EngineEffect::Preload(request)))
    }

    fn set_crossfade(mut self, crossfade: Crossfade) -> (Engine, EngineEffect) {
        self.config.crossfade = crossfade;
        let Some(mut playing) = self.take_playing() else {
            return (Engine::Live(self), EngineEffect::Nothing);
        };
        match (std::mem::take(&mut playing.next), crossfade.get().is_zero()) {
            (
                Next::Crossfading {
                    preload,
                    fading: false,
                },
                false,
            ) => {
                let cue = arm_cue(playing.current.total, crossfade.get());
                self.phase = Phase::Playing(Playing {
                    next: Next::Crossfading {
                        preload,
                        fading: false,
                    },
                    ..playing
                });
                (Engine::Live(self), EngineEffect::Arm { cue })
            }
            (
                Next::Crossfading {
                    preload,
                    fading: true,
                },
                true,
            ) => self.promote(preload, playing.preloading),
            (
                Next::Crossfading {
                    preload,
                    fading: false,
                },
                true,
            ) => {
                let path = preload.path;
                self.phase = Phase::Playing(Playing {
                    preloading: Some(path.clone()),
                    ..playing
                });
                let effect = EngineEffect::Batch(vec![
                    EngineEffect::Arm { cue: None },
                    EngineEffect::RestartGapless(path),
                ]);
                (Engine::Live(self), effect)
            }
            (next, _) => {
                self.phase = Phase::Playing(Playing { next, ..playing });
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }

    pub(crate) fn promote(
        mut self,
        preload: CurrentTrack,
        preloading: Option<PathBuf>,
    ) -> (Engine, EngineEffect) {
        self.phase = Phase::Playing(Playing {
            current: preload,
            next: Next::None,
            preloading,
        });
        let volume = self.volume();
        (
            Engine::Live(self),
            then_report(EngineEffect::Promote { volume }),
        )
    }

    pub(crate) fn devices_listed(
        self,
        devices: Vec<ListedDevice>,
    ) -> (Engine, EngineEffect) {
        (
            Engine::Live(self),
            EngineEffect::Send(AudioEvent::DevicesListed(devices)),
        )
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        AudioCmd,
        Bounded,
        Playback,
        domain::{DeviceName, OutputDevice, Replaygain, Speed},
        update::Machine,
    };
    use proptest::{
        prelude::{any, prop_assert_eq, proptest},
        test_runner::TestCaseError,
    };
    use rstest::rstest;

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage},
            machine::EngineError,
            phase::{Handover, Incoming, Phase},
            state::{Engine, Live, PerformedRevisions, then_report},
            test_support::{
                Cell,
                assert_cell,
                awaiting,
                cmd,
                config,
                crossfade,
                crossfade_preload,
                crossfading_idle,
                crossfading_mid_ramp,
                first,
                gapless_preload,
                handed_over_to_b,
                handing_over,
                live,
                live_with_crossfade,
                load,
                load_at,
                loaded_at,
                loading,
                loading_track,
                playing,
                playing_with_crossfade,
                preload,
                preload_at,
                preloaded_at,
                promoted,
                second,
                seconds,
                set_crossfade,
            },
        },
    };

    #[rstest]
    #[case::load_starts_a_decode(
        Engine::Live(live()),
        load("/a"),
        Cell {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
        }
    )]
    #[case::preload_is_gapless_without_crossfade(
        Engine::Live(playing()),
        preload("/b"),
        Cell {
            next: Engine::Live(awaiting(preloaded_at(playing(), first()), "/b")),
            effect: gapless_preload("/b"),
        }
    )]
    #[case::preload_opens_a_second_sink_with_crossfade(
        Engine::Live(playing_with_crossfade()),
        preload("/b"),
        Cell {
            next: Engine::Live(awaiting(
                preloaded_at(
                    playing_with_crossfade(),
                    first(),
                ),
                "/b",
            )),
            effect: crossfade_preload("/b"),
        }
    )]
    #[case::a_skip_without_a_crossfade_still_cuts(
        Engine::Live(playing()),
        load("/b"),
        Cell {
            next: Engine::Live(loaded_at(
                Live { phase: Phase::Loading(loading_track("/b")), ..live() },
                first(),
            )),
            effect: EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() },
        }
    )]
    #[case::play(
        Engine::Live(playing()),
        cmd(AudioCmd::Playback(Playback::Playing)),
        Cell { next: Engine::Live(playing()), effect: then_report(EngineEffect::Play) }
    )]
    #[case::pause(
        Engine::Live(playing()),
        cmd(AudioCmd::Playback(Playback::Paused)),
        Cell { next: Engine::Live(playing()), effect: then_report(EngineEffect::Pause) }
    )]
    #[case::seek(
        Engine::Live(playing()),
        cmd(AudioCmd::Seek(seconds(5))),
        Cell { next: Engine::Live(playing()), effect: then_report(EngineEffect::Seek(seconds(5))) }
    )]
    #[case::seek_while_idle_crossfade_rearms(
        Engine::Live(crossfading_idle()),
        cmd(AudioCmd::Seek(seconds(50))),
        Cell {
            next: Engine::Live(crossfading_idle()),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Seek(seconds(50)),
                EngineEffect::Arm { cue: Some(seconds(90)) },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::seek_back_out_of_a_crossfade_cancels_the_crossfade(
        Engine::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(50))),
        Cell {
            next: Engine::Live(crossfading_idle()),
            effect: EngineEffect::Batch(vec![
                EngineEffect::CancelCrossfade,
                EngineEffect::Seek(seconds(50)),
                EngineEffect::Arm { cue: Some(seconds(90)) },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::seek_inside_a_fade_keeps_fading(
        Engine::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(95))),
        Cell {
            next: Engine::Live(crossfading_mid_ramp()),
            effect: then_report(EngineEffect::Seek(seconds(95))),
        }
    )]
    #[case::speed(
        Engine::Live(playing()),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        Cell {
            next: Engine::Live(Live { speed: Speed::clamped(1.5), ..playing() }),
            effect: then_report(EngineEffect::SetSpeed(Speed::clamped(1.5))),
        }
    )]
    #[case::stop_clears_the_track(
        Engine::Live(playing()),
        cmd(AudioCmd::Stop),
        Cell { next: Engine::Live(live()), effect: EngineEffect::Clear }
    )]
    #[case::stop_clears_a_pending_load(
        Engine::Live(loading()),
        cmd(AudioCmd::Stop),
        Cell { next: Engine::Live(live()), effect: EngineEffect::Clear }
    )]
    #[case::stop_drops_a_crossfade_preload(
        Engine::Live(crossfading_idle()),
        cmd(AudioCmd::Stop),
        Cell {
            next: Engine::Live(live_with_crossfade(10)),
            effect: EngineEffect::Clear,
        }
    )]
    #[case::set_crossfade_is_stored(
        Engine::Live(playing()),
        set_crossfade(4),
        Cell {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(4), ..config() }, ..playing() }),
            effect: EngineEffect::Nothing,
        }
    )]
    #[case::set_crossfade_keeps_a_live_preload(
        Engine::Live(crossfading_idle()),
        set_crossfade(4),
        Cell {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(4), ..config() }, ..crossfading_idle() }),
            effect: EngineEffect::Arm { cue: Some(seconds(96)) },
        }
    )]
    #[case::crossfade_to_zero_promotes_a_louder_preload(
        Engine::Live(crossfading_mid_ramp()),
        set_crossfade(0),
        Cell {
            next: Engine::Live(promoted(crossfade(0))),
            effect: then_report(EngineEffect::Promote { volume: 1.0 }),
        }
    )]
    #[case::crossfade_to_zero_restarts_an_unfaded_preload(
        Engine::Live(crossfading_idle()),
        set_crossfade(0),
        Cell {
            next: Engine::Live(awaiting(playing(), "/b")),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Arm { cue: None },
                EngineEffect::RestartGapless("/b".into()),
            ]),
        }
    )]
    #[case::replaygain_reapplies_the_volume(
        Engine::Live(playing()),
        cmd(AudioCmd::SetReplaygain(Replaygain::On)),
        Cell {
            next: Engine::Live(Live { config: EngineConfig { replaygain: Replaygain::On, ..config() }, ..playing() }),
            effect: EngineEffect::SetVolume(1.0),
        }
    )]
    #[case::set_device_opens_another_device(
        Engine::Live(playing()),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        Cell {
            next: Engine::Live(playing()),
            effect: EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            },
        }
    )]
    #[case::set_device_to_the_one_in_use_is_nothing(
        Engine::Live(playing()),
        cmd(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        Cell { next: Engine::Live(playing()), effect: EngineEffect::Nothing }
    )]
    #[case::list_devices(
        Engine::Live(playing()),
        cmd(AudioCmd::ListDevices),
        Cell { next: Engine::Live(playing()), effect: EngineEffect::ListDevices }
    )]
    #[case::a_skip_with_a_crossfade_retires_the_running_stream(
        Engine::Live(playing_with_crossfade()),
        load("/b"),
        Cell {
            next: Engine::Live(Live {
                phase: Phase::Handover(Handover {
                    incoming: Incoming::Loading(loading_track("/b")),
                }),
                performed: PerformedRevisions { load: first(), ..PerformedRevisions::default() },
                ..playing_with_crossfade()
            }),
            effect: EngineEffect::StartHandover { path: "/b".into(), speed: Speed::default() },
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Cell,
    ) {
        assert_cell(start, message, moved);
    }

    #[rstest]
    #[case::while_idle(live())]
    #[case::while_loading(loading())]
    #[case::while_the_skip_is_still_decoding(handing_over(Incoming::Loading(
        loading_track("/b")
    )))]
    #[case::while_the_skip_fades_in(handed_over_to_b())]
    fn a_preload_without_a_settled_track_is_refused(#[case] start: Live) {
        let expected = Engine::Live(start.clone());
        let mut state = Engine::Live(start);
        assert_eq!(
            state.update(preload("/c")),
            Err(EngineError::WhileNotPlaying("/c".into()))
        );
        assert_eq!(state, expected);
    }

    struct ReplayRow {
        start: Engine,
        first: EngineMessage,
        again: EngineMessage,
        effect: EngineEffect,
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: Engine::Live(live()),
        first: load("/a"),
        again: load("/a"),
        effect: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
    })]
    #[case::preload(ReplayRow {
        start: Engine::Live(playing()),
        first: preload("/b"),
        again: preload("/b"),
        effect: gapless_preload("/b"),
    })]
    fn a_replayed_revision_is_performed_once(#[case] row: ReplayRow) {
        let mut engine = row.start;
        assert_eq!(engine.update(row.first), Ok(row.effect));

        let before = engine.clone();
        assert_eq!(engine.update(row.again), Ok(EngineEffect::Nothing));
        assert_eq!(engine, before);
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: Engine::Live(live()),
        first: load("/a"),
        again: load_at("/b", second()),
        effect: EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() },
    })]
    #[case::preload(ReplayRow {
        start: Engine::Live(playing()),
        first: preload("/b"),
        again: preload_at("/c", second()),
        effect: gapless_preload("/c"),
    })]
    fn a_newer_revision_is_performed_again(#[case] row: ReplayRow) {
        let mut engine = row.start;
        engine.update(row.first).unwrap();
        assert_eq!(engine.update(row.again), Ok(row.effect));
    }

    fn rejected(reason: impl std::fmt::Debug) -> TestCaseError {
        TestCaseError::fail(format!(
            "the machine rejected a valid transition: {reason:?}"
        ))
    }

    proptest! {
        #[test]
        fn a_load_opens_an_outgoing_stream_only_when_it_will_fade(
            crossfade_seconds in 0u64..30,
            already_playing in any::<bool>(),
        ) {
            let base = if already_playing { playing() } else { live() };
            let starting = Live {
                config: EngineConfig {
                    crossfade: crossfade(crossfade_seconds),
                    ..config()
                },
                ..base
            };
            let mut state = Engine::Live(starting);
            state.update(load("/next")).map_err(rejected)?;
            let Engine::Live(live) = state else {
                return Err(rejected("the engine stays live across a load"));
            };
            let fades = crossfade_seconds > 0 && already_playing;
            prop_assert_eq!(matches!(live.phase, Phase::Handover(_)), fades);
        }
    }
}
