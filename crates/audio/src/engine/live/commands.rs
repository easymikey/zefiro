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
        crossfade::fade_start,
        effect::{AudioLoopCmd, EngineEffect},
        phase::{Fade, Incoming, Loading, NextTrack, Phase, Playing},
        revisions::JobRevisions,
        state::{Live, then_report},
    },
};

impl Live {
    pub(crate) fn command(
        &mut self,
        revisions: &mut JobRevisions,
        audio_cmd: AudioCmd,
    ) -> Result<AudioLoopCmd, Unhandled> {
        match audio_cmd {
            AudioCmd::Load(TrackLoad {
                revision,
                media: _media,
                decibels: _decibels,
            }) if revision <= self.executed_revisions.load => Err(Unhandled),
            AudioCmd::Load(track_load) => Ok(self.load(revisions, track_load)),
            AudioCmd::Preload(track_load) => self.preload(revisions, track_load),
            AudioCmd::CancelPreload => self.cancel_preload(),
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
                Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(
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
            AudioCmd::SetDevice(device) => self.set_device(device),
            AudioCmd::ListDevices => {
                Ok(Cmd::effect(LoopEffect::Run(AudioJob::ListDevices)))
            }
            AudioCmd::Grow {
                revision,
                downloaded,
            } => revisions.grow(revision, downloaded).map(|()| Cmd::none()),
        }
    }

    pub(crate) fn load(
        &mut self,
        revisions: &mut JobRevisions,
        track_load: TrackLoad,
    ) -> AudioLoopCmd {
        let TrackLoad {
            media,
            decibels,
            revision,
        } = track_load;
        self.executed_revisions.load = revision;
        let speed = self.speed;
        let fades =
            !self.settings.crossfade.get().is_zero() && self.phase.current().is_some();
        let loading = Loading {
            media: media.clone(),
            decibels,
            resume: None,
        };
        if fades {
            self.phase = Phase::Handover(Incoming::Loading(loading));
            return Cmd::effect(LoopEffect::Execute(EngineEffect::StartHandover(
                speed,
            )))
            .then(Cmd::effect(LoopEffect::Run(revisions.decode_job(media))));
        }
        self.phase = Phase::Loading(loading);
        Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(speed)))
            .then(Cmd::effect(LoopEffect::Run(revisions.decode_job(media))))
    }

    fn seek(&mut self, target: Duration) -> AudioLoopCmd {
        let Phase::Playing(Playing {
            current,
            next:
                NextTrack::Crossfading {
                    fade,
                    incoming: _incoming,
                },
        }) = &mut self.phase
        else {
            return then_report(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(
                target,
            ))));
        };
        let fade_start = fade_start(current.duration, self.settings.crossfade.get());
        let running = *fade == Fade::Running;
        let resets_fade_start =
            !running || fade_start.is_some_and(|fade_start| target < fade_start);
        let audio_loop_cmd: AudioLoopCmd = [
            (running && resets_fade_start).then_some(EngineEffect::CancelCrossfade),
            Some(EngineEffect::Seek(target)),
            resets_fade_start.then_some(EngineEffect::SetFadeStart(fade_start)),
            Some(EngineEffect::Report),
        ]
        .into_iter()
        .flatten()
        .map(LoopEffect::Execute)
        .collect();
        if resets_fade_start {
            *fade = Fade::Armed;
        }
        audio_loop_cmd
    }

    fn set_device(&self, device: OutputDevice) -> Result<AudioLoopCmd, Unhandled> {
        match device {
            in_use if in_use == self.settings.device => Err(Unhandled),
            other => Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Open {
                device: other,
                speed: self.speed,
            }))),
        }
    }

    fn preload(
        &mut self,
        revisions: &mut JobRevisions,
        track_load: TrackLoad,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let TrackLoad {
            media,
            decibels,
            revision,
        } = track_load;
        if revision <= self.executed_revisions.preload {
            return Err(Unhandled);
        }
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        playing.next = NextTrack::Preloading {
            media: media.clone(),
            decibels,
        };
        self.executed_revisions.preload = revision;
        Ok(Cmd::effect(LoopEffect::Run(revisions.preload_job(media))))
    }

    fn cancel_preload(&mut self) -> Result<AudioLoopCmd, Unhandled> {
        let Phase::Playing(playing) = &mut self.phase else {
            return Err(Unhandled);
        };
        let drop_preload = Cmd::effect(LoopEffect::Execute(EngineEffect::DropPreload));
        let audio_loop_cmd = match &playing.next {
            NextTrack::None => return Err(Unhandled),
            NextTrack::Crossfading {
                incoming: _incoming,
                fade: Fade::Running,
            } => return Err(Unhandled),
            NextTrack::Crossfading {
                incoming: _incoming,
                fade: Fade::Armed,
            } => Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(None)))
                .then(drop_preload),
            NextTrack::Preloading {
                media: _media,
                decibels: _decibels,
            } => drop_preload,
            NextTrack::Gapless(_incoming) => drop_preload,
        };
        playing.next = NextTrack::None;
        Ok(audio_loop_cmd)
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
        let NextTrack::Crossfading { incoming, fade } = &mut playing.next else {
            return Cmd::none();
        };
        match (*fade, crossfade.get().is_zero()) {
            (Fade::Armed, false) => {
                Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(
                    fade_start(playing.current.duration, crossfade.get()),
                )))
            }
            (Fade::Running, true) => {
                playing.promote();
                self.promoted()
            }
            (Fade::Armed, true) => {
                let media = incoming.media.clone();
                playing.next = NextTrack::Preloading {
                    media: media.clone(),
                    decibels: incoming.decibels,
                };
                Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(None)))
                    .then(Cmd::effect(LoopEffect::Execute(EngineEffect::DropPreload)))
                    .then(Cmd::effect(LoopEffect::Run(revisions.preload_job(media))))
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
    use std::sync::atomic::Ordering;

    use kernel::{
        cmd::{AudioCmd, Cmd, GrowingMedia, Media, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            device::{DeviceName, OutputDevice},
            settings::{AudioSettings, ReplayGain},
            speed::Speed,
        },
        message::AudioEvent,
        update::machine::{LoopEffect, Machine, Unhandled},
    };
    use proptest::prelude::{any, prop_assert, prop_assert_eq, proptest};
    use rstest::rstest;

    use crate::{
        deck::job::AudioJob,
        engine::{
            effect::{AudioLoopCmd, EngineEffect},
            message::{EngineMessage, SinkRole},
            phase::{Incoming, NextTrack, Phase, Playing},
            state::{Engine, EngineState, ExecutedRevisions, Live, then_report},
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
                load_with_revision,
                loading,
                loading_track,
                playing,
                playing_with_crossfade,
                preload,
                preload_with_revision,
                preloading,
                promoted,
                second,
                seconds,
                set_crossfade,
                settings,
                step,
                track_a,
                track_b,
                unhandled,
                with_load_revision,
                with_preload_revision,
            },
        },
    };

    #[rstest]
    #[case::load_starts_a_decode(
        EngineState::Live(live()),
        load("/a"),
        EngineRow {
            next: EngineState::Live(with_load_revision(loading(), first())),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/a"))),
        }
    )]
    #[case::preload_is_gapless_without_crossfade(
        EngineState::Live(playing()),
        preload("/b"),
        EngineRow {
            next: EngineState::Live(awaiting(with_preload_revision(playing(), first()), "/b")),
            effect: Ok(preloading("/b")),
        }
    )]
    #[case::preload_opens_a_second_sink_with_crossfade(
        EngineState::Live(playing_with_crossfade()),
        preload("/b"),
        EngineRow {
            next: EngineState::Live(awaiting(
                with_preload_revision(
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
            next: EngineState::Live(with_load_revision(
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
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(seconds(50)))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(Some(seconds(90)))))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))),
        }
    )]
    #[case::seek_back_out_of_a_crossfade_cancels_the_crossfade(
        EngineState::Live(crossfading_mid_ramp()),
        cmd(AudioCmd::Seek(seconds(50))),
        EngineRow {
            next: EngineState::Live(crossfading_idle()),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::CancelCrossfade)).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Seek(seconds(50))))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(Some(seconds(90)))))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::Report)))),
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
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))))}
    )]
    #[case::stop_clears_a_pending_load(
        EngineState::Live(loading()),
        cmd(AudioCmd::Stop),
        EngineRow { next: EngineState::Live(live()), effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))))}
    )]
    #[case::stop_drops_a_crossfade_preload(
        EngineState::Live(crossfading_idle()),
        cmd(AudioCmd::Stop),
        EngineRow {
            next: EngineState::Live(live_with_crossfade(10)),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default())))),
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
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(Some(seconds(96)))))),
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
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(None))).then(Cmd::effect(LoopEffect::Execute(EngineEffect::DropPreload))).then(preloading("/b"))),
        }
    )]
    #[case::set_crossfade_mid_ramp_keeps_the_running_fade(
        EngineState::Live(crossfading_mid_ramp()),
        set_crossfade(4),
        EngineRow {
            next: EngineState::Live(Live { settings: AudioSettings { crossfade: crossfade(4), ..settings() }, ..crossfading_mid_ramp() }),
            effect: Ok(Cmd::none()),
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
                executed_revisions: ExecutedRevisions { load: first(), ..ExecutedRevisions::default() },
                ..playing_with_crossfade()
            }),
            effect: Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::StartHandover(Speed::default()))).then(decoding("/b"))),
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
    #[case::while_idle(live())]
    #[case::while_loading(loading())]
    #[case::while_the_skip_is_still_decoding(handing_over(Incoming::Loading(
        loading_track("/b")
    )))]
    #[case::while_the_skip_fades_in(handed_over_to_b())]
    fn a_preload_without_a_settled_track_is_refused(#[case] live: Live) {
        let expected = EngineState::Live(live.clone());
        let mut state = EngineState::Live(live);
        assert_eq!(step(&mut state, preload("/c")).err(), Some(Unhandled));
        assert_eq!(state, expected);
    }

    #[test]
    fn the_device_in_use_is_refused() {
        let mut state = EngineState::Live(playing());
        assert_eq!(
            step(
                &mut state,
                cmd(AudioCmd::SetDevice(OutputDevice::SystemDefault)),
            )
            .err(),
            Some(Unhandled)
        );
        assert_eq!(state, EngineState::Live(playing()));
    }

    #[test]
    fn a_stale_load_is_refused() {
        let live = with_load_revision(live(), second());
        let mut state = EngineState::Live(live.clone());
        assert_eq!(step(&mut state, load("/b")).err(), Some(Unhandled));
        assert_eq!(state, EngineState::Live(live));
    }

    #[test]
    fn a_stale_preload_is_refused() {
        let live = with_preload_revision(playing(), second());
        let mut state = EngineState::Live(live.clone());
        assert_eq!(step(&mut state, preload("/b")).err(), Some(Unhandled));
        assert_eq!(state, EngineState::Live(live));
    }

    struct ReplayRow {
        engine_state: EngineState,
        first: EngineMessage,
        engine_message: EngineMessage,
        audio_loop_cmd: AudioLoopCmd,
    }

    #[rstest]
    #[case::load(ReplayRow {
        engine_state: EngineState::Live(live()),
        first: load("/a"),
        engine_message: load("/a"),
        audio_loop_cmd: Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/a")),
    })]
    #[case::preload(ReplayRow {
        engine_state: EngineState::Live(playing()),
        first: preload("/b"),
        engine_message: preload("/b"),
        audio_loop_cmd: preloading("/b"),
    })]
    fn a_replayed_revision_is_executed_once(#[case] row: ReplayRow) {
        let mut engine = row.engine_state;
        assert_same(step(&mut engine, row.first), Ok(row.audio_loop_cmd));

        let before = engine.clone();
        assert_eq!(step(&mut engine, row.engine_message).err(), Some(Unhandled));
        assert_eq!(engine, before);
    }

    #[rstest]
    #[case::load(ReplayRow {
        engine_state: EngineState::Live(live()),
        first: load("/a"),
        engine_message: load_with_revision("/b", second()),
        audio_loop_cmd: Cmd::effect(LoopEffect::Execute(EngineEffect::StartLoad(Speed::default()))).then(decoding("/b")),
    })]
    #[case::preload(ReplayRow {
        engine_state: EngineState::Live(playing()),
        first: preload("/b"),
        engine_message: preload_with_revision("/c", second()),
        audio_loop_cmd: preloading("/c"),
    })]
    fn a_newer_revision_is_executed_again(#[case] row: ReplayRow) {
        let mut engine = row.engine_state;
        assert!(step(&mut engine, row.first).is_ok());
        assert_same(
            step(&mut engine, row.engine_message),
            Ok(row.audio_loop_cmd),
        );
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
    fn a_grow_raises_the_bound_of_the_download_being_decoded_and_another_is_refused() {
        let mut engine = Engine::new(EngineState::Live(live()));

        let loaded = engine
            .transition(cmd(AudioCmd::Load(growing_load(100))))
            .unwrap();
        let grown = engine.transition(cmd(AudioCmd::Grow {
            revision: second().next(),
            downloaded: 600,
        }));
        let other = engine.transition(cmd(AudioCmd::Grow {
            revision: first(),
            downloaded: 900,
        }));
        let bounds = loaded
            .effects()
            .filter_map(|effect| {
                let LoopEffect::Run(AudioJob::Decode {
                    media_path: _media_path,
                    download: Some(download),
                    revision: _revision,
                }) = effect
                else {
                    return None;
                };
                Some(download.downloaded.load(Ordering::Acquire))
            })
            .collect::<Vec<_>>();

        assert_eq!(grown.map(|cmd| cmd.effects().count()), Ok(0));
        assert_eq!(other.err(), Some(Unhandled));
        assert_eq!(bounds, vec![600]);
    }

    fn gapless_preloaded() -> Live {
        Live {
            phase: Phase::Playing(Playing {
                current: track_a(),
                next: NextTrack::Gapless(track_b()),
            }),
            ..playing()
        }
    }

    fn drop_preload() -> AudioLoopCmd {
        Cmd::effect(LoopEffect::Execute(EngineEffect::DropPreload))
    }

    #[rstest]
    #[case::still_decoding(awaiting(playing(), "/b"), drop_preload())]
    #[case::gapless(gapless_preloaded(), drop_preload())]
    #[case::armed_crossfade(
        crossfading_idle(),
        Cmd::effect(LoopEffect::Execute(EngineEffect::SetFadeStart(None))).then(drop_preload())
    )]
    fn cancel_preload_drops_a_preload_whose_transition_has_not_begun(
        #[case] live: Live,
        #[case] audio_loop_cmd: AudioLoopCmd,
    ) {
        let mut state = EngineState::Live(live);

        assert_same(
            step(&mut state, cmd(AudioCmd::CancelPreload)),
            Ok(audio_loop_cmd),
        );
        assert_same(
            step(&mut state, EngineMessage::Finished(SinkRole::Current)),
            Ok(then_report(Cmd::message(AudioEvent::Ended))),
        );
    }

    #[rstest]
    #[case::running_crossfade(crossfading_mid_ramp())]
    #[case::no_preload(playing())]
    #[case::idle(live())]
    fn cancel_preload_leaves_a_running_transition_and_refuses_without_a_preload(
        #[case] live: Live,
    ) {
        let mut state = EngineState::Live(live.clone());

        assert_eq!(
            step(&mut state, cmd(AudioCmd::CancelPreload)).err(),
            Some(Unhandled)
        );
        assert_eq!(state, EngineState::Live(live));
    }
}
