use std::{
    collections::{HashSet, VecDeque},
    mem::discriminant,
};

use kernel::{
    cmd::{AudioCmd, Cmd, Cmds},
    domain::{revision::Revision, transport::OutputError},
    message::{AudioError, AudioEvent},
    update::machine::{LoopEffect, Machine, Unhandled, each_handled},
};

use crate::{
    AudioDriver,
    deck::{
        event::DeckEvent,
        job::AudioJob,
        source::{DecodedTrack, PreloadMode, TrackDecoder},
    },
    engine::{
        effect::{AudioLoopCmd, EngineEffect},
        message::{AudioMessage, ClosedMessage, EngineMessage, signalled},
        revisions::JobRevisions,
        state::{Closed, DeviceChoice, Engine, EngineState},
    },
    error::{Error, decode_error_of, preload_error},
};

impl Machine for AudioDriver {
    type Message = AudioMessage;
    type Effect = AudioLoopCmd;

    fn transition(
        &mut self,
        audio_message: AudioMessage,
    ) -> Result<AudioLoopCmd, Unhandled> {
        if !self.engine.job_revisions.is_current(&audio_message) {
            return Err(Unhandled);
        }
        match audio_message {
            AudioMessage::Cmds(cmds) => {
                self.engine.transition(EngineMessage::Cmds(cmds))
            }
            AudioMessage::Deck(DeckEvent::OutputLost(error)) => self.engine.lost(error),
            AudioMessage::Deck(DeckEvent::Woke(revision)) => Ok(Cmd::effect(
                LoopEffect::Execute(EngineEffect::TakeSignals(revision)),
            )),
            AudioMessage::Decoded { revision, result } => {
                self.engine.decoded(revision, result)
            }
            AudioMessage::Preloaded { revision, result } => {
                self.engine.preloaded(revision, result)
            }
            AudioMessage::DevicesListed(listed) => self.engine.transition(
                listed.map_or_else(EngineMessage::Error, EngineMessage::DevicesListed),
            ),
            AudioMessage::SignalsTaken { role, signals } => {
                each_handled(signalled(role, signals), |each| {
                    self.engine.transition(each)
                })
            }
            AudioMessage::Engine(message) => self.engine.transition(message),
            AudioMessage::Started => self
                .feed_receiver
                .take()
                .map(|feed_receiver| {
                    Cmd::effect(LoopEffect::Run(AudioJob::Feed(feed_receiver)))
                })
                .ok_or(Unhandled),
            AudioMessage::Fed => Ok(Cmd::none()),
        }
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
                | AudioCmd::SetCrossfade(_)
                | AudioCmd::SetReplayGain(_)
                | AudioCmd::SetDevice(_)
                | AudioCmd::ListDevices => true,
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

impl Machine for Engine {
    type Message = EngineMessage;
    type Effect = AudioLoopCmd;

    fn transition(
        &mut self,
        engine_message: EngineMessage,
    ) -> Result<AudioLoopCmd, Unhandled> {
        let Engine {
            state,
            job_revisions: revisions,
            device_choice: _device_choice,
        } = self;
        match (&mut *state, engine_message) {
            (_, EngineMessage::Reported(Some(position))) => {
                Ok(Cmd::message(AudioEvent::PositionReported(position)))
            }
            (_, EngineMessage::Error(error)) => self.failed(error),
            (_, EngineMessage::Interrupted(_, error)) => state.interrupted(error),
            (_, EngineMessage::DevicesListed(devices)) => {
                Ok(Cmd::message(AudioEvent::DevicesListed(devices)))
            }
            (_, EngineMessage::Opened(device_opened)) => Ok(self.opened(device_opened)),
            (_, EngineMessage::NotFound) => Ok(self.fell_back()),
            (EngineState::Closed(closed), EngineMessage::Cmds(batch)) => {
                closed.transition(ClosedMessage::Cmds(batch))
            }
            (_, EngineMessage::Reported(None))
            | (
                EngineState::Closed(_),
                EngineMessage::Decoded(_)
                | EngineMessage::Attached { .. }
                | EngineMessage::Finished(_)
                | EngineMessage::FadeStartReached
                | EngineMessage::Ramped(_),
            ) => Err(Unhandled),
            (
                EngineState::Live(_),
                EngineMessage::Attached {
                    revision,
                    preload_mode: _preload_mode,
                    duration: _duration,
                },
            ) if !revisions.is_current_preload(revision) => Err(Unhandled),
            (EngineState::Live(live), EngineMessage::Cmds(batch)) => {
                batched(batch, |cmd| live.command(revisions, cmd))
            }
            (EngineState::Live(live), EngineMessage::Decoded(duration)) => {
                live.decoded(revisions, duration)
            }
            (
                EngineState::Live(live),
                EngineMessage::Attached {
                    preload_mode,
                    duration,
                    revision: _revision,
                },
            ) => live.attached(preload_mode, duration),
            (EngineState::Live(live), EngineMessage::Finished(role)) => {
                live.finished(role)
            }
            (EngineState::Live(live), EngineMessage::FadeStartReached) => {
                live.fade_start_reached()
            }
            (EngineState::Live(live), EngineMessage::Ramped(role)) => live.ramped(role),
        }
    }
}

fn silenced(
    state: &mut EngineState,
    job_revisions: &mut JobRevisions,
    event: AudioEvent,
) -> Result<AudioLoopCmd, Unhandled> {
    match std::mem::replace(state, EngineState::Closed(Closed::default())) {
        EngineState::Live(live) => {
            job_revisions.cancel();
            *state = EngineState::Closed(live.closed());
            Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Silence))
                .then(Cmd::message(event)))
        }
        closed @ EngineState::Closed(_) => {
            *state = closed;
            Err(Unhandled)
        }
    }
}

impl Engine {
    fn decoded(
        &mut self,
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    ) -> Result<AudioLoopCmd, Unhandled> {
        match result {
            Ok(decoder) => {
                let decoded_track = DecodedTrack { revision, decoder };
                let cmd =
                    self.transition(EngineMessage::Decoded(decoded_track.duration()))?;
                Ok(
                    Cmd::effect(LoopEffect::Execute(EngineEffect::Stage(
                        decoded_track,
                    )))
                    .then(cmd),
                )
            }
            Err(error) => self.failed(decode_error_of(error)),
        }
    }

    fn preloaded(
        &mut self,
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    ) -> Result<AudioLoopCmd, Unhandled> {
        match result {
            Ok(decoder) => Ok(Cmd::effect(LoopEffect::Execute(EngineEffect::Attach {
                decoded_track: DecodedTrack { revision, decoder },
                preload_mode: self.preload_mode().ok_or(Unhandled)?,
            }))),
            Err(error) => self.failed(preload_error(error)),
        }
    }

    fn preload_mode(&self) -> Option<PreloadMode> {
        match &self.state {
            EngineState::Live(live) => live.preload_mode(),
            EngineState::Closed(_) => None,
        }
    }

    fn failed(&mut self, error: AudioError) -> Result<AudioLoopCmd, Unhandled> {
        if matches!(error, AudioError::OpenDevice { .. }) {
            self.device_choice = DeviceChoice::Requested;
        }
        let Engine {
            state,
            job_revisions: revisions,
            device_choice: _device_choice,
        } = self;
        match (&mut *state, error) {
            (_, error @ (AudioError::Seek { .. } | AudioError::ListDevices { .. })) => {
                Ok(Cmd::message(AudioEvent::Error(error)))
            }
            (EngineState::Closed(closed), error @ AudioError::OpenDevice { .. }) => {
                closed.transition(ClosedMessage::Error(error))
            }
            (
                EngineState::Closed(_),
                AudioError::Decode { .. } | AudioError::Preload { .. },
            ) => Err(Unhandled),
            (EngineState::Live(live), error @ AudioError::Decode { .. }) => {
                live.decode_failed(revisions, error)
            }
            (EngineState::Live(live), error @ AudioError::Preload { .. }) => {
                live.preload_failed(error)
            }
            (EngineState::Live(_), error @ AudioError::OpenDevice { .. }) => {
                silenced(state, revisions, AudioEvent::Error(error))
            }
        }
    }

    fn lost(&mut self, error: OutputError) -> Result<AudioLoopCmd, Unhandled> {
        let Engine {
            state,
            job_revisions: revisions,
            device_choice: _device_choice,
        } = self;
        silenced(state, revisions, AudioEvent::OutputLost(error))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use kernel::{
        cmd::{AudioCmd, Cmds, Effect, Playback},
        domain::{
            bounded::Bounded,
            crossfade::Crossfade,
            cursor::Cursor,
            model::Model,
            playlist::Playlist,
            revision::Revision,
            settings::AudioSettings,
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track, TrackParts},
        },
        message::{AudioEvent, BrowseRequest, Message, PlaybackRequest},
        update::{
            machine::{LoopEffect, Machine, Unhandled},
            update,
        },
    };
    use rstest::rstest;

    use crate::{
        deck::{
            event::DeckEvent,
            job::AudioJob,
            source::PreloadMode,
            tests::track as source_track,
        },
        engine::{
            effect::{AudioLoopCmd, EngineEffect},
            machine::keep_last_idempotent,
            message::{AudioMessage, EngineMessage, Signals, SinkRole, signalled},
            revisions::JobRevisions,
            state::{Engine, EngineState, Live},
            tests::{
                TRACK_A_DURATION,
                awaiting,
                cmd,
                driver_with,
                executed,
                first,
                live,
                playing,
                playing_with_crossfade,
                settings,
                step,
            },
        },
    };

    const CROSSFADE: Duration = Duration::from_secs(7);

    fn worker_panicked() -> crate::error::Error {
        crate::error::Error::WorkerPanicked(PathBuf::from("/a"))
    }

    #[rstest]
    #[case::output_lost(
        AudioMessage::Deck(DeckEvent::OutputLost(
            kernel::domain::transport::OutputError::DeviceGone
        )),
        Engine::new(EngineState::Live(live())).lost(kernel::domain::transport::OutputError::DeviceGone)
    )]
    #[case::decode_error(
        AudioMessage::Decoded { revision: Revision::default(), result: Err(worker_panicked()) },
        step(&mut EngineState::Live(live()), EngineMessage::Error(crate::error::decode_error_of(worker_panicked())))
    )]
    #[case::preload_error(
        AudioMessage::Preloaded { revision: Revision::default(), result: Err(worker_panicked()) },
        step(&mut EngineState::Live(live()), EngineMessage::Error(crate::error::preload_error(worker_panicked())))
    )]
    fn a_landed_error_goes_to_the_engine_without_touching_the_deck(
        #[case] audio_message: AudioMessage,
        #[case] expected: Result<AudioLoopCmd, Unhandled>,
    ) {
        assert_eq!(
            executed(driver_with(EngineState::Live(live())).transition(audio_message)),
            executed(expected)
        );
    }

    #[test]
    fn a_command_batch_answers_alike_on_the_driver_and_engine_paths() {
        let audio_cmd = AudioCmd::SetSpeed(Speed::clamped(1.5));
        let audio_message = AudioMessage::from(Cmds {
            cmds: vec![audio_cmd.clone()],
            at: std::time::Instant::now(),
        });
        assert_eq!(
            executed(driver_with(EngineState::Live(live())).transition(audio_message)),
            executed(step(&mut EngineState::Live(live()), cmd(audio_cmd)))
        );
    }

    #[test]
    fn a_track_event_asks_execute_to_take_its_signals() {
        let audio_message = AudioMessage::Deck(DeckEvent::Woke(Revision::default()));
        assert_eq!(
            executed(driver_with(EngineState::Live(live())).transition(audio_message)),
            Ok((vec![EngineEffect::TakeSignals(Revision::default())], vec![]))
        );
    }

    #[rstest]
    #[case::stale_decode(live(), AudioMessage::Decoded { revision: first(), result: Err(worker_panicked()) })]
    #[case::stale_preload_error(awaiting(playing(), "/b"), AudioMessage::Preloaded { revision: first(), result: Err(worker_panicked()) })]
    #[case::stale_preload(awaiting(playing(), "/b"), AudioMessage::Preloaded { revision: first(), result: Ok(source_track(first()).decoder) })]
    #[case::preload_while_playing(playing(), AudioMessage::Preloaded { revision: Revision::default(), result: Ok(source_track(Revision::default()).decoder) })]
    fn an_unawaited_answer_is_refused(
        #[case] live: Live,
        #[case] audio_message: AudioMessage,
    ) {
        assert_eq!(
            executed(driver_with(EngineState::Live(live)).transition(audio_message)),
            Err(Unhandled)
        );
    }

    #[rstest]
    #[case::gapless(awaiting(playing(), "/b"), PreloadMode::Gapless)]
    #[case::crossfade(
        Live { speed: Speed::clamped(1.5), ..awaiting(playing_with_crossfade(), "/b") },
        PreloadMode::Crossfade(Speed::clamped(1.5))
    )]
    fn preloaded_ok_attaches_by_mode(#[case] live: Live, #[case] mode: PreloadMode) {
        let revision = Revision::default();
        let audio_message = AudioMessage::Preloaded {
            revision,
            result: Ok(source_track(revision).decoder),
        };
        let engine_effect = EngineEffect::Attach {
            decoded_track: source_track(revision),
            preload_mode: mode,
        };
        assert_eq!(
            executed(driver_with(EngineState::Live(live)).transition(audio_message)),
            Ok((vec![engine_effect], vec![]))
        );
    }

    #[rstest]
    #[case::fade_start_reached_current(
        SinkRole::Current,
        Signals::FADE_START,
        "[FadeStartReached]"
    )]
    #[case::fade_start_reached_incoming(SinkRole::Incoming, Signals::FADE_START, "[]")]
    #[case::ramped_outgoing(SinkRole::Outgoing, Signals::RAMPED, "[Ramped(Outgoing)]")]
    #[case::finished_current(
        SinkRole::Current,
        Signals::FINISHED,
        "[Finished(Current)]"
    )]
    #[case::none(SinkRole::Current, Signals::default(), "[]")]
    fn taken_signals_expand_into_messages(
        #[case] role: SinkRole,
        #[case] signals: Signals,
        #[case] expected: &str,
    ) {
        assert_eq!(format!("{:?}", signalled(role, signals)), expected);
    }

    #[test]
    fn taken_signals_reach_the_engine_as_one_message_each() {
        let taken_audio_message = AudioMessage::SignalsTaken {
            role: SinkRole::Current,
            signals: Signals::FINISHED,
        };
        assert_eq!(
            executed(
                driver_with(EngineState::Live(live())).transition(taken_audio_message)
            ),
            executed(step(
                &mut EngineState::Live(live()),
                EngineMessage::Finished(SinkRole::Current)
            ))
        );
    }

    fn track(number: usize) -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: format!("/tmp/track{number}.flac").into(),
            duration: TRACK_A_DURATION,
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        }))
    }

    fn listening() -> Model {
        Model {
            playlist: Playlist {
                tracks: (0..3).map(track).collect(),
                cursor: Cursor::new(3),
                ..Playlist::default()
            },
            ..Model::default()
        }
    }

    fn engine_with_crossfade() -> Engine {
        Engine::new(EngineState::Live(Live {
            settings: AudioSettings {
                crossfade: Crossfade::clamped(CROSSFADE),
                ..settings()
            },
            ..live()
        }))
    }

    fn as_audio(effect: &Effect) -> Option<AudioCmd> {
        match effect {
            Effect::Audio(audio) => Some(audio.clone()),
            Effect::Library(_)
            | Effect::Macos(_)
            | Effect::Config(_)
            | Effect::Animate(_)
            | Effect::RollShuffle(..)
            | Effect::WindowColors(_)
            | Effect::After { .. }
            | Effect::Restart(_)
            | Effect::Quit => None,
        }
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Sinks {
        current_path: Option<PathBuf>,
        incoming_path: Option<PathBuf>,
        outgoing_path: Option<PathBuf>,
    }

    impl Sinks {
        fn open(&self) -> usize {
            usize::from(self.current_path.is_some())
                + usize::from(self.incoming_path.is_some())
                + usize::from(self.outgoing_path.is_some())
        }

        fn execute(&mut self, loop_cmd: &AudioLoopCmd) {
            for effect in loop_cmd.effects() {
                match effect {
                    LoopEffect::Execute(effect) => self.run(effect),
                    LoopEffect::Run(AudioJob::Decode { path, .. }) => {
                        self.current_path = Some(path.clone());
                    }
                    LoopEffect::Run(AudioJob::Preload { path, .. }) => {
                        self.incoming_path = Some(path.clone());
                    }
                    LoopEffect::Run(AudioJob::ListDevices | AudioJob::Feed(_))
                    | LoopEffect::After { .. }
                    | LoopEffect::Watch { .. }
                    | LoopEffect::Unwatch(_) => {}
                }
            }
        }

        fn run(&mut self, engine_effect: &EngineEffect) {
            match engine_effect {
                EngineEffect::Clear(_)
                | EngineEffect::Silence
                | EngineEffect::StartLoad(_) => {
                    self.current_path = None;
                    self.incoming_path = None;
                    self.outgoing_path = None;
                }
                EngineEffect::StartHandover(_) => {
                    self.outgoing_path = self.current_path.take();
                    self.incoming_path = None;
                }
                EngineEffect::Promote(_) => {
                    self.current_path = self.incoming_path.take();
                }
                EngineEffect::DropOutgoing => {
                    self.outgoing_path = None;
                }
                EngineEffect::Open { .. }
                | EngineEffect::ClearStaged
                | EngineEffect::Start(_)
                | EngineEffect::Resume { .. }
                | EngineEffect::Play
                | EngineEffect::Pause
                | EngineEffect::Seek(_)
                | EngineEffect::SetGain(_)
                | EngineEffect::SetFadeStart(_)
                | EngineEffect::Crossfade { .. }
                | EngineEffect::CancelCrossfade
                | EngineEffect::Ramp { .. }
                | EngineEffect::SetSpeed(_)
                | EngineEffect::DropPreload
                | EngineEffect::Report
                | EngineEffect::Advance(_)
                | EngineEffect::Stage(_)
                | EngineEffect::Attach { .. }
                | EngineEffect::TakeSignals(_) => {}
            }
        }
    }

    struct Wiring {
        model: Model,
        engine: Engine,
        sinks: Sinks,
        log: Vec<Result<<crate::AudioDriver as Machine>::Effect, Unhandled>>,
    }

    impl Wiring {
        fn new() -> Self {
            Self {
                model: listening(),
                engine: engine_with_crossfade(),
                sinks: Sinks::default(),
                log: Vec::new(),
            }
        }

        fn engine_step(&mut self, engine_message: EngineMessage) {
            self.engine.job_revisions = JobRevisions::default();
            let effect = self.engine.transition(engine_message);
            if let Ok(handled) = effect.as_ref() {
                self.sinks.execute(handled);
            }
            self.log.push(effect);
        }

        fn press(&mut self, message: Message) {
            let produced = update(&mut self.model, message, Moment::default()).unwrap();
            let audio_cmds: Vec<AudioCmd> =
                produced.iter().filter_map(as_audio).collect();
            for command in audio_cmds {
                self.engine_step(cmd(command));
            }
        }

        fn decoded(&mut self) {
            self.engine_step(EngineMessage::Decoded(Some(TRACK_A_DURATION)));
            let started = self.log.last().is_some_and(|logged| {
                logged.as_ref().is_ok_and(|handled| {
                    matches!(
                        handled.effects().as_slice(),
                        [LoopEffect::Execute(EngineEffect::Start(_))]
                    )
                })
            });
            if started {
                self.press(Message::Audio(AudioEvent::Loaded(Some(TRACK_A_DURATION))));
            }
        }

        fn handover_settles(&mut self) {
            self.engine_step(EngineMessage::Ramped(SinkRole::Outgoing));
        }

        fn at_most_two_streams(&self, named: &str) {
            assert!(
                self.sinks.open() <= 2,
                "{named} must never hold more than two streams, got {:?}",
                self.sinks
            );
        }

        fn one_open_stream(&self, named: &str) {
            assert_eq!(
                self.sinks.open(),
                1,
                "{named} must leave one stream open, got {:?}",
                self.sinks
            );
        }

        fn playing_is(&self, path: &str) {
            assert_eq!(
                self.sinks.current_path,
                Some(PathBuf::from(path)),
                "the open stream must be {path}, got {:?}",
                self.sinks.current_path
            );
        }
    }

    fn load(path: &str) -> AudioCmd {
        AudioCmd::Load(kernel::cmd::TrackLoad {
            path: PathBuf::from(path),
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
            load("/b"),
            AudioCmd::Seek(Duration::from_secs(2)),
            AudioCmd::Seek(Duration::from_secs(3)),
        ],
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            load("/b"),
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
        assert_eq!(keep_last_idempotent(audio_cmds), expected);
    }

    #[test]
    fn a_picked_row_fades_over_the_stream_the_media_key_started() {
        let mut wiring = Wiring::new();

        wiring.press(Message::Playback(PlaybackRequest::Play));
        wiring.decoded();
        wiring.one_open_stream("the media key's own start");

        wiring.model.workspace.browse.cursor = Cursor::at(3, 2);
        wiring.press(Message::Browse(BrowseRequest::PlaySelected));
        wiring.at_most_two_streams("a row picked while the media key's track plays");
        wiring.playing_is("/tmp/track2.flac");
        wiring.decoded();
        wiring.handover_settles();

        wiring.one_open_stream("a finished skip fade");
        wiring.playing_is("/tmp/track2.flac");
        insta::assert_debug_snapshot!(wiring.log);
    }

    #[test]
    fn a_picked_row_fades_over_a_stream_that_is_still_decoding() {
        let mut wiring = Wiring::new();

        wiring.press(Message::Playback(PlaybackRequest::Play));
        wiring.model.workspace.browse.cursor = Cursor::at(3, 2);
        wiring.press(Message::Browse(BrowseRequest::PlaySelected));
        wiring.at_most_two_streams("a row picked mid-load");
        wiring.decoded();
        wiring.handover_settles();

        wiring.one_open_stream("a row picked mid-load");
        wiring.playing_is("/tmp/track2.flac");
        insta::assert_debug_snapshot!(wiring.log);
    }

    #[test]
    fn three_skips_inside_one_crossfade_end_on_a_single_stream() {
        let mut wiring = Wiring::new();
        wiring.model.playlist.tracks = (0..4).map(track).collect();
        wiring.model.playlist.cursor = Cursor::new(4);

        wiring.press(Message::Playback(PlaybackRequest::Play));
        wiring.decoded();

        for _ in 1..=3 {
            wiring.press(Message::Playback(PlaybackRequest::Next));
            wiring.at_most_two_streams("a skip inside a fade");
            wiring.decoded();
            wiring.at_most_two_streams("a fade running under the next skip");
        }

        wiring.handover_settles();
        wiring.one_open_stream("three skips inside one fade");
        insta::assert_debug_snapshot!(wiring.log);
    }
}
