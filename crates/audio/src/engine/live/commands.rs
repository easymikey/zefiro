use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioFailure,
    EngineRejection,
    Playback,
    domain::{Crossfade, Delivery, DeviceName, OutputDevice, Revision},
    update::Rejected,
};

use crate::engine::{
    crossfade::{Promotion, arm_cue, promotion_on_abandon},
    effect::{EngineEffect, PreloadedTrack, devices_fact},
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
    state::{Engine, Live, PendingLoad, Transition, reported},
};

struct Request {
    path: PathBuf,
    gain: Option<f32>,
    revision: Revision,
}

impl Live {
    pub(crate) fn command(mut self, cmd: AudioCmd) -> Transition {
        match cmd {
            AudioCmd::Load {
                path,
                gain,
                revision,
            } => Transition::from(self.load(PendingLoad {
                path,
                gain,
                revision,
            })),
            AudioCmd::Preload {
                path,
                gain,
                revision,
            } => self.preload(Request {
                path,
                gain,
                revision,
            }),
            AudioCmd::Pause(playback) => {
                let io = match playback {
                    Playback::Paused => EngineEffect::Pause,
                    Playback::Playing => EngineEffect::Play,
                };
                Transition::Next(Engine::Live(self), reported(io))
            }
            AudioCmd::Seek(target) => Transition::from(self.seek(target)),
            AudioCmd::SetSpeed(speed) => {
                self.speed = speed;
                Transition::Next(
                    Engine::Live(self),
                    reported(EngineEffect::SetSpeed(speed)),
                )
            }
            AudioCmd::Stop => {
                self.phase = Phase::Idle;
                Transition::Next(Engine::Live(self), EngineEffect::Clear)
            }
            AudioCmd::SetCrossfade(crossfade) => {
                Transition::from(self.set_crossfade(crossfade))
            }
            AudioCmd::SetReplaygain(replaygain) => {
                self.config.replaygain = replaygain;
                let volume = self.volume();
                Transition::Next(Engine::Live(self), EngineEffect::SetVolume(volume))
            }
            AudioCmd::SetDevice(device) => Transition::from(self.set_device(device)),
            AudioCmd::ListDevices => {
                Transition::Next(Engine::Live(self), EngineEffect::ListDevices)
            }
        }
    }

    pub(crate) fn load(mut self, pending: PendingLoad) -> (Engine, EngineEffect) {
        let PendingLoad {
            path,
            gain,
            revision,
        } = pending;
        if let Delivery::Replay = revision.delivery(self.performed.load) {
            return (Engine::Live(self), EngineEffect::Nothing);
        }
        self.performed.load = revision;
        let speed = self.speed;
        let fades =
            !self.config.crossfade.value().is_zero() && self.phase.current().is_some();
        let loading = Loading {
            path: path.clone(),
            gain,
            after_load: AfterLoad::None,
        };
        if fades {
            self.phase = Phase::Handover(Handover {
                outgoing: Outgoing { from: 0.0 },
                incoming: Incoming::Loading(loading),
            });
            return (Engine::Live(self), EngineEffect::StartFade { path, speed });
        }
        self.phase = Phase::Loading(loading);
        (Engine::Live(self), EngineEffect::StartLoad { path, speed })
    }

    fn seek(mut self, target: Duration) -> (Engine, EngineEffect) {
        let playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return (Engine::Live(self), reported(EngineEffect::Seek(target)));
            }
        };
        let Next::Crossfading { preload, fade } = playing.next else {
            self.phase = Phase::Playing(playing);
            return (Engine::Live(self), reported(EngineEffect::Seek(target)));
        };
        let cue = arm_cue(playing.current.total, self.config.crossfade.value());
        match fade {
            Fade::Idle => {
                self.phase = Phase::Playing(Playing {
                    next: Next::Crossfading {
                        preload,
                        fade: Fade::Idle,
                    },
                    ..playing
                });
                let io = EngineEffect::Many(vec![
                    EngineEffect::Seek(target),
                    EngineEffect::Arm { cue },
                    EngineEffect::Report,
                ]);
                (Engine::Live(self), io)
            }
            Fade::Fading if cue.is_some_and(|cue| target < cue) => {
                self.phase = Phase::Playing(Playing {
                    next: Next::Crossfading {
                        preload,
                        fade: Fade::Idle,
                    },
                    ..playing
                });
                let io = EngineEffect::Many(vec![
                    EngineEffect::Unfade,
                    EngineEffect::Seek(target),
                    EngineEffect::Arm { cue },
                    EngineEffect::Report,
                ]);
                (Engine::Live(self), io)
            }
            Fade::Fading => {
                self.phase = Phase::Playing(Playing {
                    next: Next::Crossfading {
                        preload,
                        fade: Fade::Fading,
                    },
                    ..playing
                });
                (Engine::Live(self), reported(EngineEffect::Seek(target)))
            }
        }
    }

    fn set_device(self, device: Option<DeviceName>) -> (Engine, EngineEffect) {
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

    fn preload(mut self, requested: Request) -> Transition {
        let Request {
            path,
            gain,
            revision,
        } = requested;
        if let Delivery::Replay = revision.delivery(self.performed.preload) {
            return Transition::Next(Engine::Live(self), EngineEffect::Nothing);
        }
        let playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return Transition::Rejected(Rejected {
                    state: Engine::Live(self),
                    reason: EngineRejection::WhileNotPlaying(path),
                });
            }
        };
        self.performed.preload = revision;
        self.phase = Phase::Playing(Playing {
            preloading: Some(path.clone()),
            ..playing
        });
        let io = if self.config.crossfade.value().is_zero() {
            EngineEffect::PreloadGapless(path)
        } else {
            EngineEffect::PreloadCrossfade {
                path,
                gain,
                speed: self.speed,
            }
        };
        Transition::Next(Engine::Live(self), io)
    }

    fn set_crossfade(mut self, crossfade: Crossfade) -> (Engine, EngineEffect) {
        self.config.crossfade = crossfade;
        let mut playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return (Engine::Live(self), EngineEffect::Nothing);
            }
        };
        match (
            std::mem::take(&mut playing.next),
            crossfade.value().is_zero(),
        ) {
            (
                Next::Crossfading {
                    preload,
                    fade: Fade::Idle,
                },
                false,
            ) => {
                let cue = arm_cue(playing.current.total, crossfade.value());
                self.phase = Phase::Playing(Playing {
                    next: Next::Crossfading {
                        preload,
                        fade: Fade::Idle,
                    },
                    ..playing
                });
                (Engine::Live(self), EngineEffect::Arm { cue })
            }
            (Next::Crossfading { preload, fade }, true) => {
                match promotion_on_abandon(fade) {
                    Promotion::Preload => self.promote(preload, playing.preloading),
                    Promotion::Nothing => {
                        let path = preload.path;
                        self.phase = Phase::Playing(Playing {
                            preloading: Some(path.clone()),
                            ..playing
                        });
                        let io = EngineEffect::Many(vec![
                            EngineEffect::Arm { cue: None },
                            EngineEffect::RestartGapless(path),
                        ]);
                        (Engine::Live(self), io)
                    }
                }
            }
            (next, _) => {
                self.phase = Phase::Playing(Playing { next, ..playing });
                (Engine::Live(self), EngineEffect::Nothing)
            }
        }
    }

    pub(crate) fn promote(
        mut self,
        preload: PreloadedTrack,
        preloading: Option<PathBuf>,
    ) -> (Engine, EngineEffect) {
        let PreloadedTrack { path, gain, total } = preload;
        self.phase = Phase::Playing(Playing {
            current: CurrentTrack { total, gain, path },
            next: Next::None,
            preloading,
        });
        let volume = self.volume();
        (
            Engine::Live(self),
            reported(EngineEffect::Promote { volume }),
        )
    }

    pub(crate) fn devices_listed(
        self,
        result: Result<Vec<OutputDevice>, AudioFailure>,
    ) -> (Engine, EngineEffect) {
        (Engine::Live(self), EngineEffect::Send(devices_fact(result)))
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        AudioCmd,
        Bounded,
        EngineRejection,
        Playback,
        domain::{DeviceName, Replaygain, Speed},
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
            phase::{Fade, Handover, Incoming, Outgoing, Phase},
            state::{
                Engine,
                Live,
                Stamps,
                fixtures::{
                    awaiting,
                    cmd,
                    config,
                    crossfade,
                    crossfading,
                    first,
                    handing_over,
                    live,
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
                    retiring,
                    second,
                    secs,
                    set_crossfade,
                },
                reported,
            },
        },
    };

    struct Transition {
        next: Engine,
        io: EngineEffect,
    }

    #[rstest]
    #[case::load_starts_a_decode(
        Engine::Live(live()),
        load("/a"),
        Transition {
            next: Engine::Live(loaded_at(loading(), first())),
            io: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
        }
    )]
    #[case::load_supersedes_the_current_track(
        Engine::Live(playing()),
        load("/b"),
        Transition {
            next: Engine::Live(loaded_at(
                Live { phase: Phase::Loading(loading_track("/b")), ..live() },
                first(),
            )),
            io: EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() },
        }
    )]
    #[case::preload_is_gapless_without_crossfade(
        Engine::Live(playing()),
        preload("/b"),
        Transition {
            next: Engine::Live(awaiting(preloaded_at(playing(), first()), "/b")),
            io: EngineEffect::PreloadGapless("/b".into()),
        }
    )]
    #[case::preload_opens_a_second_sink_with_crossfade(
        Engine::Live(Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..playing() }),
        preload("/b"),
        Transition {
            next: Engine::Live(awaiting(
                preloaded_at(
                    Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..playing() },
                    first(),
                ),
                "/b",
            )),
            io: EngineEffect::PreloadCrossfade { path: "/b".into(), gain: None, speed: Speed::default() },
        }
    )]
    #[case::a_skip_without_a_crossfade_still_cuts(
        Engine::Live(playing()),
        load("/b"),
        Transition {
            next: Engine::Live(loaded_at(
                Live { phase: Phase::Loading(loading_track("/b")), ..live() },
                first(),
            )),
            io: EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() },
        }
    )]
    #[case::play(
        Engine::Live(playing()),
        cmd(AudioCmd::Pause(Playback::Playing)),
        Transition { next: Engine::Live(playing()), io: reported(EngineEffect::Play) }
    )]
    #[case::pause(
        Engine::Live(playing()),
        cmd(AudioCmd::Pause(Playback::Paused)),
        Transition { next: Engine::Live(playing()), io: reported(EngineEffect::Pause) }
    )]
    #[case::seek(
        Engine::Live(playing()),
        cmd(AudioCmd::Seek(secs(5))),
        Transition { next: Engine::Live(playing()), io: reported(EngineEffect::Seek(secs(5))) }
    )]
    #[case::seek_while_idle_crossfade_rearms(
        Engine::Live(crossfading(Fade::Idle)),
        cmd(AudioCmd::Seek(secs(50))),
        Transition {
            next: Engine::Live(crossfading(Fade::Idle)),
            io: EngineEffect::Many(vec![
                EngineEffect::Seek(secs(50)),
                EngineEffect::Arm { cue: Some(secs(90)) },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::seek_back_out_of_a_fade_unfades(
        Engine::Live(crossfading(Fade::Fading)),
        cmd(AudioCmd::Seek(secs(50))),
        Transition {
            next: Engine::Live(crossfading(Fade::Idle)),
            io: EngineEffect::Many(vec![
                EngineEffect::Unfade,
                EngineEffect::Seek(secs(50)),
                EngineEffect::Arm { cue: Some(secs(90)) },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::seek_inside_a_fade_keeps_fading(
        Engine::Live(crossfading(Fade::Fading)),
        cmd(AudioCmd::Seek(secs(95))),
        Transition {
            next: Engine::Live(crossfading(Fade::Fading)),
            io: reported(EngineEffect::Seek(secs(95))),
        }
    )]
    #[case::speed(
        Engine::Live(playing()),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        Transition {
            next: Engine::Live(Live { speed: Speed::clamped(1.5), ..playing() }),
            io: reported(EngineEffect::SetSpeed(Speed::clamped(1.5))),
        }
    )]
    #[case::stop_clears_the_track(
        Engine::Live(playing()),
        cmd(AudioCmd::Stop),
        Transition { next: Engine::Live(live()), io: EngineEffect::Clear }
    )]
    #[case::stop_clears_a_pending_load(
        Engine::Live(loading()),
        cmd(AudioCmd::Stop),
        Transition { next: Engine::Live(live()), io: EngineEffect::Clear }
    )]
    #[case::stop_drops_a_crossfade_preload(
        Engine::Live(crossfading(Fade::Idle)),
        cmd(AudioCmd::Stop),
        Transition {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..live() }),
            io: EngineEffect::Clear,
        }
    )]
    #[case::set_crossfade_is_stored(
        Engine::Live(playing()),
        set_crossfade(4),
        Transition {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(4), ..config() }, ..playing() }),
            io: EngineEffect::Nothing,
        }
    )]
    #[case::set_crossfade_keeps_a_live_preload(
        Engine::Live(crossfading(Fade::Idle)),
        set_crossfade(4),
        Transition {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(4), ..config() }, ..crossfading(Fade::Idle) }),
            io: EngineEffect::Arm { cue: Some(secs(96)) },
        }
    )]
    #[case::crossfade_to_zero_promotes_a_louder_preload(
        Engine::Live(crossfading(Fade::Fading)),
        set_crossfade(0),
        Transition {
            next: Engine::Live(promoted(crossfade(0))),
            io: reported(EngineEffect::Promote { volume: 1.0 }),
        }
    )]
    #[case::crossfade_to_zero_restarts_an_unfaded_preload(
        Engine::Live(crossfading(Fade::Idle)),
        set_crossfade(0),
        Transition {
            next: Engine::Live(awaiting(playing(), "/b")),
            io: EngineEffect::Many(vec![
                EngineEffect::Arm { cue: None },
                EngineEffect::RestartGapless("/b".into()),
            ]),
        }
    )]
    #[case::replaygain_reapplies_the_volume(
        Engine::Live(playing()),
        cmd(AudioCmd::SetReplaygain(Replaygain::On)),
        Transition {
            next: Engine::Live(Live { config: EngineConfig { replaygain: Replaygain::On, ..config() }, ..playing() }),
            io: EngineEffect::SetVolume(1.0),
        }
    )]
    #[case::set_device_opens_another_device(
        Engine::Live(playing()),
        cmd(AudioCmd::SetDevice(Some(DeviceName::new("usb".to_string()).unwrap()))),
        Transition {
            next: Engine::Live(playing()),
            io: EngineEffect::Open {
                device: Some(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            },
        }
    )]
    #[case::set_device_to_the_one_in_use_is_nothing(
        Engine::Live(playing()),
        cmd(AudioCmd::SetDevice(None)),
        Transition { next: Engine::Live(playing()), io: EngineEffect::Nothing }
    )]
    #[case::list_devices(
        Engine::Live(playing()),
        cmd(AudioCmd::ListDevices),
        Transition { next: Engine::Live(playing()), io: EngineEffect::ListDevices }
    )]
    #[case::a_skip_with_a_crossfade_retires_the_running_stream(
        Engine::Live(playing_with_crossfade()),
        load("/b"),
        Transition {
            next: Engine::Live(Live {
                phase: Phase::Handover(Handover {
                    outgoing: Outgoing { from: 0.0 },
                    incoming: Incoming::Loading(loading_track("/b")),
                }),
                performed: Stamps { load: first(), ..Stamps::default() },
                ..playing_with_crossfade()
            }),
            io: EngineEffect::StartFade { path: "/b".into(), speed: Speed::default() },
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Transition,
    ) {
        let mut state = start;
        let effect = state.update(message).unwrap();
        assert_eq!(state, moved.next);
        assert_eq!(effect, moved.io);
    }

    #[rstest]
    #[case::while_idle(live())]
    #[case::while_loading(loading())]
    #[case::while_the_skip_is_still_decoding(handing_over(
        Outgoing { from: 1.0 },
        Incoming::Loading(loading_track("/b")),
    ))]
    #[case::while_the_skip_fades_in(retiring(0.5))]
    fn a_preload_without_a_settled_track_is_refused(#[case] start: Live) {
        let expected = Engine::Live(start.clone());
        let mut state = Engine::Live(start);
        assert_eq!(
            state.update(preload("/c")),
            Err(EngineRejection::WhileNotPlaying("/c".into()))
        );
        assert_eq!(state, expected);
    }

    struct ReplayRow {
        start: Engine,
        first: EngineMessage,
        again: EngineMessage,
        io: EngineEffect,
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: Engine::Live(live()),
        first: load("/a"),
        again: load("/a"),
        io: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
    })]
    #[case::preload(ReplayRow {
        start: Engine::Live(playing()),
        first: preload("/b"),
        again: preload("/b"),
        io: EngineEffect::PreloadGapless("/b".into()),
    })]
    fn a_replayed_revision_is_performed_once(#[case] row: ReplayRow) {
        let mut engine = row.start;
        assert_eq!(engine.update(row.first), Ok(row.io));

        let before = engine.clone();
        assert_eq!(engine.update(row.again), Ok(EngineEffect::Nothing));
        assert_eq!(engine, before);
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: Engine::Live(live()),
        first: load("/a"),
        again: load_at("/b", second()),
        io: EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() },
    })]
    #[case::preload(ReplayRow {
        start: Engine::Live(playing()),
        first: preload("/b"),
        again: preload_at("/c", second()),
        io: EngineEffect::PreloadGapless("/c".into()),
    })]
    fn a_newer_revision_is_performed_again(#[case] row: ReplayRow) {
        let mut engine = row.start;
        engine.update(row.first).unwrap();
        assert_eq!(engine.update(row.again), Ok(row.io));
    }

    fn rejected(reason: impl std::fmt::Debug) -> TestCaseError {
        TestCaseError::fail(format!(
            "the machine rejected a valid transition: {reason:?}"
        ))
    }

    proptest! {
        #[test]
        fn a_load_opens_an_outgoing_stream_only_when_it_will_fade(
            crossfade_secs in 0u64..30,
            already_playing in any::<bool>(),
        ) {
            let base = if already_playing { playing() } else { live() };
            let starting = Live {
                config: EngineConfig {
                    crossfade: crossfade(crossfade_secs),
                    ..config()
                },
                ..base
            };
            let mut state = Engine::Live(starting);
            state.update(load("/next")).map_err(rejected)?;
            let Engine::Live(live) = state else {
                return Err(rejected("the engine stays live across a load"));
            };
            let fades = crossfade_secs > 0 && already_playing;
            prop_assert_eq!(matches!(live.phase, Phase::Handover(_)), fades);
        }
    }
}
