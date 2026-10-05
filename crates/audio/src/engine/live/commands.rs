use std::time::Duration;

use kernel::{
    cmd::{AudioCmd, Cmd, Playback, TrackLoad},
    domain::{crossfade::Crossfade, device::OutputDevice},
    message::AudioEvent,
    update::machine::{LoopEffect, Unhandled},
};

use crate::{
    deck::job::AudioJob,
    engine::{
        crossfade::arm_cue,
        effect::{AudioLoopCmd, EngineEffect},
        phase::{Fade, Incoming, Loading, Next, Phase, Playing},
        revisions::JobRevisions,
        state::{Live, then_report},
    },
};

impl Live {
    pub(crate) fn command(
        &mut self,
        revisions: &mut JobRevisions,
        cmd: AudioCmd,
    ) -> Result<AudioLoopCmd, Unhandled> {
        match cmd {
            AudioCmd::Load(load) => Ok(self.load(revisions, load)),
            AudioCmd::Preload(load) => self.preload(revisions, load),
            AudioCmd::SetPlayback(playback) => {
                let effect = match playback {
                    Playback::Paused => EngineEffect::Pause,
                    Playback::Playing => EngineEffect::Play,
                };
                Ok(then_report(Cmd::effect(LoopEffect::Execute(effect))))
            }
            AudioCmd::Seek(target) => Ok(self.seek(target)),
            AudioCmd::SetSpeed(speed) => {
                self.speed = speed;
                Ok(then_report(Cmd::effect(LoopEffect::Execute(
                    EngineEffect::SetSpeed(speed),
                ))))
            }
            AudioCmd::Stop => {
                self.phase = Phase::Idle;
                revisions.cancel();
                Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(
                    self.speed,
                ))))
            }
            AudioCmd::SetCrossfade(crossfade) => {
                Ok(self.set_crossfade(revisions, crossfade))
            }
            AudioCmd::SetReplayGain(replay_gain) => {
                self.settings.replay_gain = replay_gain;
                Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(
                    self.gain(),
                ))))
            }
            AudioCmd::SetDevice(device) => Ok(self.set_device(device)),
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(LoopEffect::Run(AudioJob::ListDevices)))
            }
        }
    }

    pub(crate) fn load(
        &mut self,
        revisions: &mut JobRevisions,
        pending: TrackLoad,
    ) -> AudioLoopCmd {
        let TrackLoad {
            path,
            gain,
            revision,
        } = pending;
        if revision <= self.executed.load {
            return Cmd::none();
        }
        self.executed.load = revision;
        let speed = self.speed;
        let fades =
            !self.settings.crossfade.get().is_zero() && self.phase.current().is_some();
        let loading = Loading {
            path: path.clone(),
            gain,
            after_load: None,
        };
        if fades {
            self.phase = Phase::Handover(Incoming::Loading(loading));
            return Cmd::effect(LoopEffect::Execute(EngineEffect::StartHandover(
                speed,
            )))
            .then(Cmd::effect(LoopEffect::Run(revisions.decode(path))));
        }
        self.phase = Phase::Loading(loading);
        Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(speed)))
            .then(Cmd::effect(LoopEffect::Run(revisions.decode(path))))
    }

    fn seek(&mut self, target: Duration) -> AudioLoopCmd {
        let Phase::Playing(Playing {
            current,
            next: Next::Crossfading { fade, .. },
        }) = &mut self.phase
        else {
            return then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(
                target,
            ))));
        };
        let cue = arm_cue(current.total, self.settings.crossfade.get());
        let running = *fade == Fade::Running;
        let rearm = !running || cue.is_some_and(|cue| target < cue);
        let steps: AudioLoopCmd = [
            (running && rearm).then_some(EngineEffect::CancelCrossfade),
            Some(EngineEffect::Seek(target)),
            rearm.then_some(EngineEffect::Arm(cue)),
            Some(EngineEffect::Report),
        ]
        .into_iter()
        .flatten()
        .map(LoopEffect::Execute)
        .collect();
        if rearm {
            *fade = Fade::Armed;
        }
        steps
    }

    fn set_device(&self, device: OutputDevice) -> AudioLoopCmd {
        match device {
            in_use if in_use == self.settings.device => Cmd::none(),
            other => Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                device: other,
                speed: self.speed,
            })),
        }
    }

    fn preload(
        &mut self,
        revisions: &mut JobRevisions,
        requested: TrackLoad,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let TrackLoad {
            path,
            gain,
            revision,
        } = requested;
        if revision <= self.executed.incoming {
            return Ok(Cmd::none());
        }
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        playing.next = Next::Preloading {
            path: path.clone(),
            gain,
        };
        self.executed.incoming = revision;
        Ok(Cmd::effect(LoopEffect::Run(revisions.preload(path))))
    }

    fn set_crossfade(
        &mut self,
        revisions: &mut JobRevisions,
        crossfade: Crossfade,
    ) -> AudioLoopCmd {
        self.settings.crossfade = crossfade;
        let Phase::Playing(playing) = &mut self.phase else {
            return Cmd::none();
        };
        let Next::Crossfading { preload, fade } = &mut playing.next else {
            return Cmd::none();
        };
        match (*fade, crossfade.get().is_zero()) {
            (Fade::Armed, false) => Cmd::effect(LoopEffect::Execute(
                EngineEffect::Arm(arm_cue(playing.current.total, crossfade.get())),
            )),
            (Fade::Running, true) => {
                playing.promote();
                self.promoted()
            }
            (Fade::Armed, true) => {
                let path = preload.path.clone();
                playing.next = Next::Preloading {
                    path: path.clone(),
                    gain: preload.gain,
                };
                Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(None)))
                    .then(Cmd::effect(LoopEffect::Execute(
                        EngineEffect::RestartGapless,
                    )))
                    .then(Cmd::effect(LoopEffect::Run(revisions.preload(path))))
            }
            (Fade::Running, false) => Cmd::none(),
        }
    }

    pub(crate) fn promoted(&self) -> AudioLoopCmd {
        then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Promote(
            self.gain(),
        ))))
        .then(Cmd::message(AudioEvent::TrackChanged))
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        cmd::{AudioCmd, Cmd, Playback},
        domain::{
            bounded::Bounded,
            device::{DeviceName, OutputDevice},
            settings::{AudioSettings, ReplayGain},
            speed::Speed,
        },
        message::AudioEvent,
        update::machine::{LoopEffect, Unhandled},
    };
    use proptest::prelude::{any, prop_assert, prop_assert_eq, proptest};
    use rstest::rstest;

    use crate::{
        deck::job::AudioJob,
        engine::{
            effect::{AudioLoopCmd, EngineEffect},
            message::EngineMessage,
            phase::{Incoming, Phase},
            state::{EngineState, ExecutedRevisions, Live, then_report},
            tests::{
                EngineRow,
                assert_cell,
                assert_same,
                awaiting,
                cmd,
                crossfade,
                crossfading_idle,
                crossfading_mid_ramp,
                decoding,
                first,
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
                preloading,
                promoted,
                second,
                seconds,
                set_crossfade,
                settings,
                step,
                unhandled,
            },
        },
    };

    #[rstest]
    #[case::load_starts_a_decode(
        EngineState::Live(live()),
        load("/a"),
        EngineRow {
            next: EngineState::Live(loaded_at(loading(), first())),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/a"))),
        }
    )]
    #[case::preload_is_gapless_without_crossfade(
        EngineState::Live(playing()),
        preload("/b"),
        EngineRow {
            next: EngineState::Live(awaiting(preloaded_at(playing(), first()), "/b")),
            effect: Ok(preloading("/b")),
        }
    )]
    #[case::preload_opens_a_second_sink_with_crossfade(
        EngineState::Live(playing_with_crossfade()),
        preload("/b"),
        EngineRow {
            next: EngineState::Live(awaiting(
                preloaded_at(
                    playing_with_crossfade(),
                    first(),
                ),
                "/b",
            )),
            effect: Ok(preloading("/b")),
        }
    )]
    #[case::a_skip_without_a_crossfade_still_cuts(
        EngineState::Live(playing()),
        load("/b"),
        EngineRow {
            next: EngineState::Live(loaded_at(
                Live { phase: Phase::Loading(loading_track("/b")), ..live() },
                first(),
            )),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/b"))),
        }
    )]
    #[case::play(
        EngineState::Live(playing()),
        cmd(AudioCmd::SetPlayback(Playback::Playing)),
        EngineRow { next: EngineState::Live(playing()), effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Play))))}
    )]
    #[case::pause(
        EngineState::Live(playing()),
        cmd(AudioCmd::SetPlayback(Playback::Paused)),
        EngineRow { next: EngineState::Live(playing()), effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Pause))))}
    )]
    #[case::seek(
        EngineState::Live(playing()),
        cmd(AudioCmd::Seek(seconds(5))),
        EngineRow { next: EngineState::Live(playing()), effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(seconds(5))))))}
    )]
    #[case::seek_while_idle_crossfade_rearms(
        EngineState::Live(crossfading_idle()),
        cmd(AudioCmd::Seek(seconds(50))),
        EngineRow {
            next: EngineState::Live(crossfading_idle()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(seconds(50)))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(Some(seconds(90)))))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))),
        }
    )]
    #[case::seek_back_out_of_a_crossfade_cancels_the_crossfade(
        EngineState::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(50))),
        EngineRow {
            next: EngineState::Live(crossfading_idle()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::CancelCrossfade)).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(seconds(50))))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(Some(seconds(90)))))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))),
        }
    )]
    #[case::seek_inside_a_fade_keeps_fading(
        EngineState::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(95))),
        EngineRow {
            next: EngineState::Live(crossfading_mid_ramp()),
            effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(seconds(95)))))),
        }
    )]
    #[case::speed(
        EngineState::Live(playing()),
        cmd(AudioCmd::SetSpeed(Speed::clamped(1.5))),
        EngineRow {
            next: EngineState::Live(Live { speed: Speed::clamped(1.5), ..playing() }),
            effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::SetSpeed(Speed::clamped(1.5)))))),
        }
    )]
    #[case::stop_clears_the_track(
        EngineState::Live(playing()),
        cmd(AudioCmd::Stop),
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(Speed::default()))))}
    )]
    #[case::stop_clears_a_pending_load(
        EngineState::Live(loading()),
        cmd(AudioCmd::Stop),
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(Speed::default()))))}
    )]
    #[case::stop_drops_a_crossfade_preload(
        EngineState::Live(crossfading_idle()),
        cmd(AudioCmd::Stop),
        EngineRow {
            next: EngineState::Live(live_with_crossfade(10)),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Clear(Speed::default())))),
        }
    )]
    #[case::set_crossfade_is_stored(
        EngineState::Live(playing()),
        set_crossfade(4),
        EngineRow {
            next: EngineState::Live(Live { settings: AudioSettings { crossfade: crossfade(4), ..settings() }, ..playing() }),
            effect: Ok(Cmd::none()),
        }
    )]
    #[case::set_crossfade_keeps_a_live_preload(
        EngineState::Live(crossfading_idle()),
        set_crossfade(4),
        EngineRow {
            next: EngineState::Live(Live { settings: AudioSettings { crossfade: crossfade(4), ..settings() }, ..crossfading_idle() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(Some(seconds(96)))))),
        }
    )]
    #[case::crossfade_to_zero_promotes_a_louder_preload(
        EngineState::Live(crossfading_mid_ramp()),
        set_crossfade(0),
        EngineRow {
            next: EngineState::Live(promoted(crossfade(0))),
            effect: Ok(then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Promote(crate::gain::Gain::UNITY)))).then(Cmd::message(AudioEvent::TrackChanged))),
        }
    )]
    #[case::crossfade_to_zero_restarts_an_unfaded_preload(
        EngineState::Live(crossfading_idle()),
        set_crossfade(0),
        EngineRow {
            next: EngineState::Live(awaiting(playing(), "/b")),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Arm(None))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::RestartGapless))).then(preloading("/b"))),
        }
    )]
    #[case::replay_gain_reapplies_the_gain(
        EngineState::Live(playing()),
        cmd(AudioCmd::SetReplayGain(ReplayGain::On)),
        EngineRow {
            next: EngineState::Live(Live { settings: AudioSettings { replay_gain: ReplayGain::On, ..settings() }, ..playing() }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetGain(crate::gain::Gain::UNITY)))),
        }
    )]
    #[case::set_device_opens_another_device(
        EngineState::Live(playing()),
        cmd(AudioCmd::SetDevice(OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()))),
        EngineRow {
            next: EngineState::Live(playing()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                device: OutputDevice::Named(DeviceName::new("usb".to_string()).unwrap()),
                speed: Speed::default(),
            }))),
        }
    )]
    #[case::set_device_to_the_one_in_use_is_nothing(
        EngineState::Live(playing()),
        cmd(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
        EngineRow { next: EngineState::Live(playing()), effect: Ok(Cmd::none())}
    )]
    #[case::list_devices(
        EngineState::Live(playing()),
        cmd(AudioCmd::ListDevices),
        EngineRow { next: EngineState::Live(playing()), effect: Ok(Cmd::effect(LoopEffect::Run(AudioJob::ListDevices)))}
    )]
    #[case::a_skip_with_a_crossfade_retires_the_running_stream(
        EngineState::Live(playing_with_crossfade()),
        load("/b"),
        EngineRow {
            next: EngineState::Live(Live {
                phase: Phase::Handover(Incoming::Loading(loading_track("/b"))),
                executed: ExecutedRevisions { load: first(), ..ExecutedRevisions::default() },
                ..playing_with_crossfade()
            }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartHandover(Speed::default()))).then(decoding("/b"))),
        }
    )]
    fn a_cell_moves_the_engine_and_names_its_io(
        #[case] start: EngineState,
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
        let expected = EngineState::Live(start.clone());
        let mut state = EngineState::Live(start);
        assert_same(step(&mut state, preload("/c")), Err(Unhandled));
        assert_eq!(state, expected);
    }

    struct ReplayRow {
        start: EngineState,
        first: EngineMessage,
        again: EngineMessage,
        effect: AudioLoopCmd,
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: EngineState::Live(live()),
        first: load("/a"),
        again: load("/a"),
        effect: Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/a")),
    })]
    #[case::preload(ReplayRow {
        start: EngineState::Live(playing()),
        first: preload("/b"),
        again: preload("/b"),
        effect: preloading("/b"),
    })]
    fn a_replayed_revision_is_executed_once(#[case] row: ReplayRow) {
        let mut engine = row.start;
        assert_same(step(&mut engine, row.first), Ok(row.effect));

        let before = engine.clone();
        assert_same(step(&mut engine, row.again), Ok(Cmd::none()));
        assert_eq!(engine, before);
    }

    #[rstest]
    #[case::load(ReplayRow {
        start: EngineState::Live(live()),
        first: load("/a"),
        again: load_at("/b", second()),
        effect: Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/b")),
    })]
    #[case::preload(ReplayRow {
        start: EngineState::Live(playing()),
        first: preload("/b"),
        again: preload_at("/c", second()),
        effect: preloading("/c"),
    })]
    fn a_newer_revision_is_executed_again(#[case] row: ReplayRow) {
        let mut engine = row.start;
        assert!(step(&mut engine, row.first).is_ok());
        assert_same(step(&mut engine, row.again), Ok(row.effect));
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
            let mut state = EngineState::Live(starting);
            prop_assert!(step(&mut state, load("/next")).is_ok());
            let EngineState::Live(live) = state else {
                return Err(unhandled("the engine stays live across a load"));
            };
            let fades = crossfade_seconds > 0 && already_playing;
            prop_assert_eq!(matches!(live.phase, Phase::Handover(_)), fades);
        }
    }
}
