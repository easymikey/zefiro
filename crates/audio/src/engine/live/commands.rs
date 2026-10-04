use std::time::Duration;

use kernel::{
    AudioCmd,
    AudioEvent,
    Cmd,
    Playback,
    TrackLoad,
    domain::{Crossfade, OutputDevice},
    update::Unhandled,
};

use crate::{
    deck::{job::AudioJob, source::PreloadMode},
    engine::{
        crossfade::arm_cue,
        effect::EngineEffect,
        phase::{Fade, Handover, Incoming, Loading, Next, Phase, Playing},
        state::{Live, then_report},
    },
};

impl Live {
    pub(crate) fn command(
        &mut self,
        cmd: AudioCmd,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match cmd {
            AudioCmd::Load(load) => Ok(self.load(load)),
            AudioCmd::Preload(load) => self.preload(load),
            AudioCmd::Playback(playback) => {
                let effect = match playback {
                    Playback::Paused => EngineEffect::Pause,
                    Playback::Playing => EngineEffect::Play,
                };
                Ok(then_report(Cmd::effect(effect)))
            }
            AudioCmd::Seek(target) => Ok(self.seek(target)),
            AudioCmd::SetSpeed(speed) => {
                self.speed = speed;
                Ok(then_report(Cmd::effect(EngineEffect::SetSpeed(speed))))
            }
            AudioCmd::Stop => {
                self.phase = Phase::Idle;
                Ok(Cmd::effect(EngineEffect::Clear(self.speed)))
            }
            AudioCmd::SetCrossfade(crossfade) => Ok(self.set_crossfade(crossfade)),
            AudioCmd::SetReplayGain(replay_gain) => {
                self.settings.replay_gain = replay_gain;
                Ok(Cmd::effect(EngineEffect::SetGain(self.gain())))
            }
            AudioCmd::SetDevice(device) => Ok(self.set_device(device)),
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(EngineEffect::Run(AudioJob::ListDevices)))
            }
        }
    }

    pub(crate) fn load(&mut self, pending: TrackLoad) -> Cmd<EngineEffect, AudioEvent> {
        let TrackLoad {
            path,
            gain,
            revision,
        } = pending;
        if revision <= self.performed.load {
            return Cmd::none();
        }
        self.performed.load = revision;
        let speed = self.speed;
        let fades =
            !self.settings.crossfade.get().is_zero() && self.phase.current().is_some();
        let loading = Loading {
            path: path.clone(),
            gain,
            after_load: None,
        };
        if fades {
            self.phase = Phase::Handover(Handover {
                incoming: Incoming::Loading(loading),
            });
            return Cmd::effect(EngineEffect::StartHandover { path, speed });
        }
        self.phase = Phase::Loading(loading);
        Cmd::effect(EngineEffect::StartLoad { path, speed })
    }

    fn seek(&mut self, target: Duration) -> Cmd<EngineEffect, AudioEvent> {
        let Phase::Playing(Playing {
            current,
            next: Next::Crossfading { fade, .. },
        }) = &mut self.phase
        else {
            return then_report(Cmd::effect(EngineEffect::Seek(target)));
        };
        let cue = arm_cue(current.total, self.settings.crossfade.get());
        let running = *fade == Fade::Running;
        let rearm = !running || cue.is_some_and(|cue| target < cue);
        let steps: Cmd<EngineEffect, AudioEvent> = [
            (running && rearm).then_some(EngineEffect::CancelCrossfade),
            Some(EngineEffect::Seek(target)),
            rearm.then_some(EngineEffect::Arm(cue)),
            Some(EngineEffect::Report),
        ]
        .into_iter()
        .flatten()
        .collect();
        if rearm {
            *fade = Fade::Armed;
        }
        steps
    }

    fn set_device(&self, device: OutputDevice) -> Cmd<EngineEffect, AudioEvent> {
        match device {
            in_use if in_use == self.settings.device => Cmd::none(),
            other => Cmd::effect(EngineEffect::Open {
                device: other,
                speed: self.speed,
            }),
        }
    }

    fn preload(
        &mut self,
        requested: TrackLoad,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        let TrackLoad {
            path,
            gain,
            revision,
        } = requested;
        if revision <= self.performed.incoming {
            return Ok(Cmd::none());
        }
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        playing.next = Next::Preloading(path.clone());
        self.performed.incoming = revision;
        let mode = if self.settings.crossfade.get().is_zero() {
            PreloadMode::Gapless
        } else {
            PreloadMode::Crossfade {
                gain,
                speed: self.speed,
            }
        };
        Ok(Cmd::effect(EngineEffect::Preload { path, mode }))
    }

    fn set_crossfade(&mut self, crossfade: Crossfade) -> Cmd<EngineEffect, AudioEvent> {
        self.settings.crossfade = crossfade;
        let Phase::Playing(playing) = &mut self.phase else {
            return Cmd::none();
        };
        let Next::Crossfading { preload, fade } = &mut playing.next else {
            return Cmd::none();
        };
        match (*fade, crossfade.get().is_zero()) {
            (Fade::Armed, false) => Cmd::effect(EngineEffect::Arm(arm_cue(
                playing.current.total,
                crossfade.get(),
            ))),
            (Fade::Running, true) => {
                playing.promote();
                self.promoted()
            }
            (Fade::Armed, true) => {
                let path = preload.path.clone();
                playing.next = Next::Preloading(path.clone());
                Cmd::effect(EngineEffect::Arm(None))
                    .then(Cmd::effect(EngineEffect::RestartGapless(path)))
            }
            (Fade::Running, false) => Cmd::none(),
        }
    }

    pub(crate) fn promoted(&self) -> Cmd<EngineEffect, AudioEvent> {
        then_report(Cmd::effect(EngineEffect::Promote(self.gain())))
            .then(Cmd::message(AudioEvent::TrackChanged))
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        AudioCmd,
        AudioEvent,
        Bounded,
        Cmd,
        Playback,
        domain::{AudioSettings, DeviceName, OutputDevice, ReplayGain, Speed},
        update::{Machine, Unhandled},
    };
    use proptest::prelude::{any, prop_assert, prop_assert_eq, proptest};
    use rstest::rstest;

    use crate::engine::{
        effect::{EngineEffect, EngineMessage},
        phase::{Handover, Incoming, Phase},
        state::{Engine, Live, PerformedRevisions, then_report},
        tests::{
            EngineRow,
            assert_cell,
            awaiting,
            cmd,
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
            settings,
            unhandled,
        },
    };

    #[rstest]
    #[case::load_starts_a_decode(
        Engine::Live(live()),
        load("/a"),
        EngineRow {
            next: Engine::Live(loaded_at(loading(), first())),
            effect: Cmd::effect(EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() }),
        }
    )]
    #[case::preload_is_gapless_without_crossfade(
        Engine::Live(playing()),
        preload("/b"),
        EngineRow {
            next: Engine::Live(awaiting(preloaded_at(playing(), first()), "/b")),
            effect: gapless_preload("/b"),
        }
    )]
    #[case::preload_opens_a_second_sink_with_crossfade(
        Engine::Live(playing_with_crossfade()),
        preload("/b"),
        EngineRow {
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
        EngineRow {
            next: Engine::Live(loaded_at(
                Live { phase: Phase::Loading(loading_track("/b")), ..live() },
                first(),
            )),
            effect: Cmd::effect(EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() }),
        }
    )]
    #[case::play(
        Engine::Live(playing()),
        cmd(AudioCmd::Playback(Playback::Playing)),
        EngineRow { next: Engine::Live(playing()), effect: then_report(Cmd::effect(EngineEffect::Play))}
    )]
    #[case::pause(
        Engine::Live(playing()),
        cmd(AudioCmd::Playback(Playback::Paused)),
        EngineRow { next: Engine::Live(playing()), effect: then_report(Cmd::effect(EngineEffect::Pause))}
    )]
    #[case::seek(
        Engine::Live(playing()),
        cmd(AudioCmd::Seek(seconds(5))),
        EngineRow { next: Engine::Live(playing()), effect: then_report(Cmd::effect(EngineEffect::Seek(seconds(5))))}
    )]
    #[case::seek_while_idle_crossfade_rearms(
        Engine::Live(crossfading_idle()),
        cmd(AudioCmd::Seek(seconds(50))),
        EngineRow {
            next: Engine::Live(crossfading_idle()),
            effect: Cmd::effect(EngineEffect::Seek(seconds(50))).then(Cmd::effect(EngineEffect::Arm(Some(seconds(90))))).then(Cmd::effect(EngineEffect::Report)),
        }
    )]
    #[case::seek_back_out_of_a_crossfade_cancels_the_crossfade(
        Engine::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(50))),
        EngineRow {
            next: Engine::Live(crossfading_idle()),
            effect: Cmd::effect(EngineEffect::CancelCrossfade).then(Cmd::effect(EngineEffect::Seek(seconds(50)))).then(Cmd::effect(EngineEffect::Arm(Some(seconds(90))))).then(Cmd::effect(EngineEffect::Report)),
        }
    )]
    #[case::seek_inside_a_fade_keeps_fading(
        Engine::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(95))),
        EngineRow {
            next: Engine::Live(crossfading_mid_ramp()),
            effect: then_report(Cmd::effect(EngineEffect::Seek(seconds(95)))),
        }
    )]
    #[case::speed(
        Engine::Live(playing()),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        EngineRow {
            next: Engine::Live(Live { speed: Speed::clamped(1.5), ..playing() }),
            effect: then_report(Cmd::effect(EngineEffect::SetSpeed(Speed::clamped(1.5)))),
        }
    )]
    #[case::stop_clears_the_track(
        Engine::Live(playing()),
        cmd(AudioCmd::Stop),
        EngineRow { next: Engine::Live(live()), effect: Cmd::effect(EngineEffect::Clear(Speed::default()))}
    )]
    #[case::stop_clears_a_pending_load(
        Engine::Live(loading()),
        cmd(AudioCmd::Stop),
        EngineRow { next: Engine::Live(live()), effect: Cmd::effect(EngineEffect::Clear(Speed::default()))}
    )]
    #[case::stop_drops_a_crossfade_preload(
        Engine::Live(crossfading_idle()),
        cmd(AudioCmd::Stop),
        EngineRow {
            next: Engine::Live(live_with_crossfade(10)),
            effect: Cmd::effect(EngineEffect::Clear(Speed::default())),
        }
    )]
    #[case::set_crossfade_is_stored(
        Engine::Live(playing()),
        set_crossfade(4),
        EngineRow {
            next: Engine::Live(Live { settings: AudioSettings { crossfade: crossfade(4), ..settings() }, ..playing() }),
            effect: Cmd::none(),
        }
    )]
    #[case::set_crossfade_keeps_a_live_preload(
        Engine::Live(crossfading_idle()),
        set_crossfade(4),
        EngineRow {
            next: Engine::Live(Live { settings: AudioSettings { crossfade: crossfade(4), ..settings() }, ..crossfading_idle() }),
            effect: Cmd::effect(EngineEffect::Arm(Some(seconds(96)))),
        }
    )]
    #[case::crossfade_to_zero_promotes_a_louder_preload(
        Engine::Live(crossfading_mid_ramp()),
        set_crossfade(0),
        EngineRow {
            next: Engine::Live(promoted(crossfade(0))),
            effect: then_report(Cmd::effect(EngineEffect::Promote(crate::gain::Gain::UNITY))).then(Cmd::message(AudioEvent::TrackChanged)),
        }
    )]
    #[case::crossfade_to_zero_restarts_an_unfaded_preload(
        Engine::Live(crossfading_idle()),
        set_crossfade(0),
        EngineRow {
            next: Engine::Live(awaiting(playing(), "/b")),
            effect: Cmd::effect(EngineEffect::Arm(None)).then(Cmd::effect(EngineEffect::RestartGapless("/b".into()))),
        }
    )]
    #[case::replay_gain_reapplies_the_gain(
        Engine::Live(playing()),
        cmd(AudioCmd::SetReplayGain(ReplayGain::On)),
        EngineRow {
            next: Engine::Live(Live { settings: AudioSettings { replay_gain: ReplayGain::On, ..settings() }, ..playing() }),
            effect: Cmd::effect(EngineEffect::SetGain(crate::gain::Gain::UNITY)),
        }
    )]
    #[case::set_device_opens_another_device(
        Engine::Live(playing()),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        EngineRow {
            next: Engine::Live(playing()),
            effect: Cmd::effect(EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            }),
        }
    )]
    #[case::set_device_to_the_one_in_use_is_nothing(
        Engine::Live(playing()),
        cmd(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        EngineRow { next: Engine::Live(playing()), effect: Cmd::none()}
    )]
    #[case::list_devices(
        Engine::Live(playing()),
        cmd(AudioCmd::ListDevices),
        EngineRow { next: Engine::Live(playing()), effect: Cmd::effect(EngineEffect::Run(crate::deck::job::AudioJob::ListDevices))}
    )]
    #[case::a_skip_with_a_crossfade_retires_the_running_stream(
        Engine::Live(playing_with_crossfade()),
        load("/b"),
        EngineRow {
            next: Engine::Live(Live {
                phase: Phase::Handover(Handover {
                    incoming: Incoming::Loading(loading_track("/b")),
                }),
                performed: PerformedRevisions { load: first(), ..PerformedRevisions::default() },
                ..playing_with_crossfade()
            }),
            effect: Cmd::effect(EngineEffect::StartHandover { path: "/b".into(), speed: Speed::default() }),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: Engine,
        #[case] message: EngineMessage,
        #[case] moved: EngineRow,
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
        assert_eq!(state.transition(preload("/c")), Err(Unhandled));
        assert_eq!(state, expected);
    }

    struct ReplayRow {
        start: Engine,
        first: EngineMessage,
        again: EngineMessage,
        effect: Cmd<EngineEffect, AudioEvent>,
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: Engine::Live(live()),
        first: load("/a"),
        again: load("/a"),
        effect: Cmd::effect(EngineEffect::StartLoad { path: "/a".into(), speed: Speed::default() }),
    })]
    #[case::preload(ReplayRow {
        start: Engine::Live(playing()),
        first: preload("/b"),
        again: preload("/b"),
        effect: gapless_preload("/b"),
    })]
    fn a_replayed_revision_is_performed_once(#[case] row: ReplayRow) {
        let mut engine = row.start;
        assert_eq!(engine.transition(row.first), Ok(row.effect));

        let before = engine.clone();
        assert_eq!(engine.transition(row.again), Ok(Cmd::none()));
        assert_eq!(engine, before);
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: Engine::Live(live()),
        first: load("/a"),
        again: load_at("/b", second()),
        effect: Cmd::effect(EngineEffect::StartLoad { path: "/b".into(), speed: Speed::default() }),
    })]
    #[case::preload(ReplayRow {
        start: Engine::Live(playing()),
        first: preload("/b"),
        again: preload_at("/c", second()),
        effect: gapless_preload("/c"),
    })]
    fn a_newer_revision_is_performed_again(#[case] row: ReplayRow) {
        let mut engine = row.start;
        assert!(engine.transition(row.first).is_ok());
        assert_eq!(engine.transition(row.again), Ok(row.effect));
    }

    proptest! {
        #[test]
        fn a_load_opens_an_outgoing_stream_only_when_it_will_fade(
            crossfade_seconds in 0u64..30,
            already_playing in any::<bool>(),
        ) {
            let base = if already_playing { playing() } else { live() };
            let starting = Live {
                settings: AudioSettings {
                    crossfade: crossfade(crossfade_seconds),
                    ..settings()
                },
                ..base
            };
            let mut state = Engine::Live(starting);
            prop_assert!(state.transition(load("/next")).is_ok());
            let Engine::Live(live) = state else {
                return Err(unhandled("the engine stays live across a load"));
            };
            let fades = crossfade_seconds > 0 && already_playing;
            prop_assert_eq!(matches!(live.phase, Phase::Handover(_)), fades);
        }
    }
}
