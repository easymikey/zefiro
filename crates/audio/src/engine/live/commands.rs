use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioError,
    EngineError,
    Playback,
    domain::{Crossfade, ListedDevice, OutputDevice},
    update::Rejected,
};

use crate::engine::{
    crossfade::{Promotion, arm_cue, promotion_on_abandon},
    effect::{EngineEffect, PreloadedTrack, devices_event},
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
    state::{Engine, Live, TrackRequest, Transition, reported},
};

impl Live {
    pub(crate) fn command(mut self, cmd: AudioCmd) -> Transition {
        match cmd {
            AudioCmd::Load {
                path,
                gain,
                revision,
            } => Transition::from(self.load(TrackRequest {
                path,
                gain,
                revision,
            })),
            AudioCmd::Preload {
                path,
                gain,
                revision,
            } => self.preload(TrackRequest {
                path,
                gain,
                revision,
            }),
            AudioCmd::Playback(playback) => {
                let effect = match playback {
                    Playback::Paused => EngineEffect::Pause,
                    Playback::Playing => EngineEffect::Play,
                };
                Transition::Next(Engine::Live(self), reported(effect))
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
                let effect = EngineEffect::Batch(vec![
                    EngineEffect::Seek(target),
                    EngineEffect::Arm { cue },
                    EngineEffect::Report,
                ]);
                (Engine::Live(self), effect)
            }
            Fade::Fading if cue.is_some_and(|cue| target < cue) => {
                self.phase = Phase::Playing(Playing {
                    next: Next::Crossfading {
                        preload,
                        fade: Fade::Idle,
                    },
                    ..playing
                });
                let effect = EngineEffect::Batch(vec![
                    EngineEffect::Unfade,
                    EngineEffect::Seek(target),
                    EngineEffect::Arm { cue },
                    EngineEffect::Report,
                ]);
                (Engine::Live(self), effect)
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

    fn preload(mut self, requested: TrackRequest) -> Transition {
        let TrackRequest {
            path,
            gain,
            revision,
        } = requested;
        if revision.is_stale(self.performed.incoming) {
            return Transition::Next(Engine::Live(self), EngineEffect::Nothing);
        }
        let playing = match std::mem::take(&mut self.phase) {
            Phase::Playing(playing) => playing,
            phase @ (Phase::Idle | Phase::Loading(_) | Phase::Handover(_)) => {
                self.phase = phase;
                return Transition::Rejected(Rejected {
                    state: Engine::Live(self),
                    reason: EngineError::WhileNotPlaying(path),
                });
            }
        };
        self.performed.incoming = revision;
        self.phase = Phase::Playing(Playing {
            preloading: Some(path.clone()),
            ..playing
        });
        let effect = if self.config.crossfade.value().is_zero() {
            EngineEffect::PreloadGapless(path)
        } else {
            EngineEffect::PreloadCrossfade {
                path,
                gain,
                speed: self.speed,
            }
        };
        Transition::Next(Engine::Live(self), effect)
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
                        let effect = EngineEffect::Batch(vec![
                            EngineEffect::Arm { cue: None },
                            EngineEffect::RestartGapless(path),
                        ]);
                        (Engine::Live(self), effect)
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
        result: Result<Vec<ListedDevice>, AudioError>,
    ) -> (Engine, EngineEffect) {
        (
            Engine::Live(self),
            EngineEffect::Send(devices_event(result)),
        )
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        AudioCmd,
        Bounded,
        EngineError,
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
            phase::{Fade, Handover, Incoming, Outgoing, Phase},
            state::{
                Engine,
                Live,
                PerformedRevisions,
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
                    seconds,
                    set_crossfade,
                },
                reported,
            },
        },
    };

    struct Cell {
        next: Engine,
        effect: EngineEffect,
    }

    #[rstest]
    #[case::load_starts_a_decode(
        Engine::Live(live()),
        load("/a"),
        Cell {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() },
        }
    )]
    #[case::load_supersedes_the_current_track(
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
    #[case::preload_is_gapless_without_crossfade(
        Engine::Live(playing()),
        preload("/b"),
        Cell {
            next: Engine::Live(awaiting(preloaded_at(playing(), first()), "/b")),
            effect: EngineEffect::PreloadGapless("/b".into()),
        }
    )]
    #[case::preload_opens_a_second_sink_with_crossfade(
        Engine::Live(Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..playing() }),
        preload("/b"),
        Cell {
            next: Engine::Live(awaiting(
                preloaded_at(
                    Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..playing() },
                    first(),
                ),
                "/b",
            )),
            effect: EngineEffect::PreloadCrossfade { path: "/b".into(), gain: None, speed: Speed::default() },
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
        Cell { next: Engine::Live(playing()), effect: reported(EngineEffect::Play) }
    )]
    #[case::pause(
        Engine::Live(playing()),
        cmd(AudioCmd::Playback(Playback::Paused)),
        Cell { next: Engine::Live(playing()), effect: reported(EngineEffect::Pause) }
    )]
    #[case::seek(
        Engine::Live(playing()),
        cmd(AudioCmd::Seek(seconds(5))),
        Cell { next: Engine::Live(playing()), effect: reported(EngineEffect::Seek(seconds(5))) }
    )]
    #[case::seek_while_idle_crossfade_rearms(
        Engine::Live(crossfading(Fade::Idle)),
        cmd(AudioCmd::Seek(seconds(50))),
        Cell {
            next: Engine::Live(crossfading(Fade::Idle)),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Seek(seconds(50)),
                EngineEffect::Arm { cue: Some(seconds(90)) },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::seek_back_out_of_a_fade_unfades(
        Engine::Live(crossfading(Fade::Fading)),
        cmd(AudioCmd::Seek(seconds(50))),
        Cell {
            next: Engine::Live(crossfading(Fade::Idle)),
            effect: EngineEffect::Batch(vec![
                EngineEffect::Unfade,
                EngineEffect::Seek(seconds(50)),
                EngineEffect::Arm { cue: Some(seconds(90)) },
                EngineEffect::Report,
            ]),
        }
    )]
    #[case::seek_inside_a_fade_keeps_fading(
        Engine::Live(crossfading(Fade::Fading)),
        cmd(AudioCmd::Seek(seconds(95))),
        Cell {
            next: Engine::Live(crossfading(Fade::Fading)),
            effect: reported(EngineEffect::Seek(seconds(95))),
        }
    )]
    #[case::speed(
        Engine::Live(playing()),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        Cell {
            next: Engine::Live(Live { speed: Speed::clamped(1.5), ..playing() }),
            effect: reported(EngineEffect::SetSpeed(Speed::clamped(1.5))),
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
        Engine::Live(crossfading(Fade::Idle)),
        cmd(AudioCmd::Stop),
        Cell {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(10), ..config() }, ..live() }),
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
        Engine::Live(crossfading(Fade::Idle)),
        set_crossfade(4),
        Cell {
            next: Engine::Live(Live { config: EngineConfig { crossfade: crossfade(4), ..config() }, ..crossfading(Fade::Idle) }),
            effect: EngineEffect::Arm { cue: Some(seconds(96)) },
        }
    )]
    #[case::crossfade_to_zero_promotes_a_louder_preload(
        Engine::Live(crossfading(Fade::Fading)),
        set_crossfade(0),
        Cell {
            next: Engine::Live(promoted(crossfade(0))),
            effect: reported(EngineEffect::Promote { volume: 1.0 }),
        }
    )]
    #[case::crossfade_to_zero_restarts_an_unfaded_preload(
        Engine::Live(crossfading(Fade::Idle)),
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
                    outgoing: Outgoing { from: 0.0 },
                    incoming: Incoming::Loading(loading_track("/b")),
                }),
                performed: PerformedRevisions { load: first(), ..PerformedRevisions::default() },
                ..playing_with_crossfade()
            }),
            effect: EngineEffect::StartFade { path: "/b".into(), speed: Speed::default() },
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: Cell,
    ) {
        let mut state = start;
        let effect = state.update(message).unwrap();
        assert_eq!(state, moved.next);
        assert_eq!(effect, moved.effect);
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
        effect: EngineEffect::PreloadGapless("/b".into()),
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
        effect: EngineEffect::PreloadGapless("/c".into()),
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
