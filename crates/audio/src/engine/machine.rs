use std::{
    collections::{HashSet, VecDeque},
    mem::discriminant,
};

use kernel::{
    AudioCmd,
    AudioError,
    AudioEvent,
    Cmd,
    Cmds,
    update::{Machine, Unhandled},
};

use crate::{
    AudioDriver,
    deck::{envelope::Signals, event::DeckEvent, source::TrackSource},
    engine::{
        effect::{AudioMessage, EngineEffect, EngineMessage, SinkRole},
        state::Engine,
    },
    error::preload_error,
};

impl Machine for AudioDriver {
    type Message = AudioMessage;
    type Effect = Cmd<EngineEffect, AudioEvent>;

    fn transition(
        &mut self,
        message: AudioMessage,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        let cmd = match message {
            AudioMessage::Deck(event) if !self.revisions.current(&event) => Cmd::none(),
            AudioMessage::Deck(event) => self.landed(event)?,
            AudioMessage::SignalsTaken { role, signals } => {
                each_handled(signalled(role, signals), |each| {
                    self.engine.transition(each)
                })?
            }
            AudioMessage::Cmds(batch) => {
                self.engine.transition(EngineMessage::Cmds(batch))?
            }
            AudioMessage::Reported(playhead) => {
                self.engine.transition(EngineMessage::Reported(playhead))?
            }
            AudioMessage::Error(error) => {
                self.engine.transition(EngineMessage::Error(error))?
            }
            AudioMessage::Opened(opened) => {
                self.engine.transition(EngineMessage::Opened(opened))?
            }
            AudioMessage::Decoded(decoded) => {
                self.engine.transition(EngineMessage::Decoded(decoded))?
            }
            AudioMessage::Preloaded(preloaded) => self
                .engine
                .transition(EngineMessage::Preloaded(preloaded))?,
            AudioMessage::Finished(role) => {
                self.engine.transition(EngineMessage::Finished(role))?
            }
            AudioMessage::Cued => self.engine.transition(EngineMessage::Cued)?,
            AudioMessage::Ramped(role) => {
                self.engine.transition(EngineMessage::Ramped(role))?
            }
            AudioMessage::DevicesListed(listed) => self
                .engine
                .transition(EngineMessage::DevicesListed(listed))?,
        };
        Ok(self.revisions.with_jobs(cmd))
    }
}

impl AudioDriver {
    fn landed(
        &mut self,
        event: DeckEvent,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match event {
            DeckEvent::OutputLost(kind) => self
                .engine
                .transition(EngineMessage::Error(AudioError::OutputLost(kind))),
            DeckEvent::DevicesListed(listed) => self.engine.transition(
                listed.map_or_else(EngineMessage::Error, EngineMessage::DevicesListed),
            ),
            DeckEvent::Decoded {
                revision,
                result: Ok(source),
            } => {
                let track = TrackSource { revision, source };
                let decoded = self
                    .engine
                    .transition(EngineMessage::Decoded(track.total()))?;
                Ok(Cmd::effect(EngineEffect::Stage(track)).then(decoded))
            }
            DeckEvent::Decoded {
                result: Err(error), ..
            } => self
                .engine
                .transition(EngineMessage::Error(AudioError::from(&error))),
            DeckEvent::Preloaded {
                revision,
                result: Ok(source),
            } => Ok(Cmd::effect(EngineEffect::Attach(TrackSource {
                revision,
                source,
            }))),
            DeckEvent::Preloaded {
                result: Err(error), ..
            } => self
                .engine
                .transition(EngineMessage::Error(preload_error(&error))),
            DeckEvent::Woke(revision) => {
                Ok(Cmd::effect(EngineEffect::TakeSignals(revision)))
            }
        }
    }
}

pub(crate) fn signalled(role: SinkRole, signals: Signals) -> Vec<EngineMessage> {
    [
        (signals.contains(Signals::CUED) && role == SinkRole::Primary)
            .then_some(EngineMessage::Cued),
        signals
            .contains(Signals::RAMPED)
            .then_some(EngineMessage::Ramped(role)),
        signals
            .contains(Signals::FINISHED)
            .then_some(EngineMessage::Finished(role)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn each_handled<T>(
    asked: Vec<T>,
    mut run: impl FnMut(T) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled>,
) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
    let count = asked.len();
    let (handled, effect) = asked
        .into_iter()
        .filter_map(|each| run(each).ok())
        .fold((0, Cmd::none()), |(handled, all), next| {
            (handled + 1, all.then(next))
        });
    if count > 0 && handled == 0 {
        Err(Unhandled)
    } else {
        Ok(effect)
    }
}

impl Machine for Engine {
    type Message = EngineMessage;
    type Effect = Cmd<EngineEffect, AudioEvent>;

    fn transition(
        &mut self,
        message: EngineMessage,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        self.step(message)
    }
}

pub(crate) fn keep_last_idempotent(batch: Vec<AudioCmd>) -> Vec<AudioCmd> {
    let (_seen, kept) = batch.into_iter().rev().fold(
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
                AudioCmd::Playback(_)
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
    batch: Cmds<AudioCmd>,
    run: impl FnMut(AudioCmd) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled>,
) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
    each_handled(keep_last_idempotent(batch.cmds), run)
}

impl Engine {
    fn step(
        &mut self,
        message: EngineMessage,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match (&mut *self, message) {
            (_, EngineMessage::Reported(playhead)) => Ok(playhead
                .map_or_else(Cmd::none, |position| {
                    Cmd::message(AudioEvent::Playhead(position))
                })),
            (_, EngineMessage::Error(error)) => self.failed(error),
            (_, EngineMessage::DevicesListed(devices)) => {
                Ok(Cmd::message(AudioEvent::DevicesListed(devices)))
            }
            (Engine::Closed(closed), EngineMessage::Opened(reopened)) => {
                let (live, effect) = closed.reopened(reopened);
                *self = Engine::Live(live);
                Ok(effect)
            }
            (
                Engine::Closed(closed),
                message @ (EngineMessage::Cmds(_)
                | EngineMessage::Decoded(_)
                | EngineMessage::Preloaded(_)
                | EngineMessage::Finished(_)
                | EngineMessage::Cued
                | EngineMessage::Ramped(_)),
            ) => closed.transition(message),
            (Engine::Live(live), EngineMessage::Cmds(batch)) => {
                batched(batch, |cmd| live.command(cmd))
            }
            (Engine::Live(live), EngineMessage::Opened(reopened)) => {
                Ok(live.opened(reopened))
            }
            (Engine::Live(live), EngineMessage::Decoded(total)) => live.decoded(total),
            (Engine::Live(live), EngineMessage::Preloaded(preload)) => {
                live.preloaded(preload)
            }
            (Engine::Live(live), EngineMessage::Finished(role)) => live.finished(role),
            (Engine::Live(live), EngineMessage::Cued) => live.cued(),
            (Engine::Live(live), EngineMessage::Ramped(role)) => live.ramped(role),
        }
    }

    fn failed(
        &mut self,
        error: AudioError,
    ) -> Result<Cmd<EngineEffect, AudioEvent>, Unhandled> {
        match (&mut *self, error) {
            (_, error @ (AudioError::Seek { .. } | AudioError::ListDevices { .. })) => {
                Ok(Cmd::message(AudioEvent::Error(error)))
            }
            (
                Engine::Closed(closed),
                error @ (AudioError::Device { .. }
                | AudioError::Decode { .. }
                | AudioError::Preload { .. }
                | AudioError::Stream { .. }
                | AudioError::OutputLost(_)),
            ) => closed.transition(EngineMessage::Error(error)),
            (Engine::Live(live), error @ AudioError::Decode { .. }) => {
                live.decode_failed(error)
            }
            (Engine::Live(live), error @ AudioError::Preload { .. }) => {
                live.preload_failed(error)
            }
            (
                Engine::Live(live),
                error @ (AudioError::Device { .. }
                | AudioError::Stream { .. }
                | AudioError::OutputLost(_)),
            ) => {
                let effect = Cmd::effect(EngineEffect::Mute)
                    .then(Cmd::message(AudioEvent::Error(error)));
                *self = Engine::Closed(live.failed());
                Ok(effect)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use kernel::{
        AudioCmd,
        AudioEvent,
        Bounded,
        BrowseRequest,
        Cmd,
        Effect,
        Message,
        Model,
        Moment,
        Playback,
        PlaybackRequest,
        Playlist,
        Track,
        domain::{
            AudioFormat,
            AudioSettings,
            Crossfade,
            Cursor,
            Revision,
            Speed,
            Tags,
        },
        update::{Machine, Unhandled, update},
    };
    use rstest::rstest;

    use crate::{
        deck::{envelope::Signals, event::DeckEvent},
        engine::{
            effect::{AudioMessage, EngineEffect, EngineMessage, SinkRole},
            machine::{keep_last_idempotent, signalled},
            state::{Engine, Live},
            tests::{TOTAL, cmd, live, settings},
        },
    };

    const CROSSFADE: Duration = Duration::from_secs(7);

    fn live_driver() -> crate::AudioDriver {
        let (spectrum, _tap) = crate::tap::new_tap();
        let (sender, _heard) = crossbeam_channel::bounded(4);
        crate::AudioDriver {
            engine: Engine::Live(live()),
            revisions: crate::engine::revisions::Revisions::default(),
            deck: crate::deck::Deck::new(spectrum, sender),
        }
    }

    fn worker_panicked() -> crate::error::Error {
        crate::error::Error::WorkerPanicked(PathBuf::from("/a"))
    }

    #[rstest]
    #[case::output_lost(
        DeckEvent::OutputLost(kernel::domain::StreamError::DeviceGone),
        EngineMessage::Error(kernel::AudioError::OutputLost(
            kernel::domain::StreamError::DeviceGone
        ))
    )]
    #[case::decode_error(
        DeckEvent::Decoded { revision: Revision::default(), result: Err(worker_panicked()) },
        EngineMessage::Error(kernel::AudioError::from(&worker_panicked()))
    )]
    #[case::preload_error(
        DeckEvent::Preloaded { revision: Revision::default(), result: Err(worker_panicked()) },
        EngineMessage::Error(crate::error::preload_error(&worker_panicked()))
    )]
    fn a_landed_error_goes_to_the_engine_without_touching_the_deck(
        #[case] event: DeckEvent,
        #[case] routed: EngineMessage,
    ) {
        assert_eq!(
            live_driver().transition(AudioMessage::Deck(event)),
            Engine::Live(live()).transition(routed)
        );
    }

    #[test]
    fn a_track_event_asks_execute_to_take_its_signals() {
        let event = DeckEvent::Woke(Revision::default());
        assert_eq!(
            live_driver().transition(AudioMessage::Deck(event)),
            Ok(Cmd::effect(EngineEffect::TakeSignals(Revision::default())))
        );
    }

    #[test]
    fn a_stale_decode_is_dropped() {
        let event = DeckEvent::Decoded {
            revision: Revision::default().next(),
            result: Err(worker_panicked()),
        };
        assert_eq!(
            live_driver().transition(AudioMessage::Deck(event)),
            Ok(Cmd::none())
        );
    }

    #[rstest]
    #[case::cued_primary(SinkRole::Primary, Signals::CUED, "[Cued]")]
    #[case::cued_incoming(SinkRole::Incoming, Signals::CUED, "[]")]
    #[case::ramped_outgoing(SinkRole::Outgoing, Signals::RAMPED, "[Ramped(Outgoing)]")]
    #[case::finished_primary(
        SinkRole::Primary,
        Signals::FINISHED,
        "[Finished(Primary)]"
    )]
    #[case::none(SinkRole::Primary, Signals::default(), "[]")]
    fn taken_signals_expand_into_messages(
        #[case] role: SinkRole,
        #[case] signals: Signals,
        #[case] expected: &str,
    ) {
        assert_eq!(format!("{:?}", signalled(role, signals)), expected);
    }

    #[test]
    fn taken_signals_reach_the_engine_as_one_message_each() {
        let taken = AudioMessage::SignalsTaken {
            role: SinkRole::Primary,
            signals: Signals::FINISHED,
        };
        assert_eq!(
            live_driver().transition(taken),
            Engine::Live(live()).transition(EngineMessage::Finished(SinkRole::Primary))
        );
    }

    fn track(number: usize) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("/tmp/track{number}.flac"))
                .duration(TOTAL)
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        )
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
        Engine::Live(Live {
            settings: AudioSettings {
                crossfade: Crossfade::clamped(CROSSFADE),
                ..settings()
            },
            ..live()
        })
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
        primary: Option<PathBuf>,
        preload: Option<PathBuf>,
        outgoing: Option<PathBuf>,
    }

    impl Sinks {
        fn open(&self) -> usize {
            usize::from(self.primary.is_some())
                + usize::from(self.preload.is_some())
                + usize::from(self.outgoing.is_some())
        }

        fn execute(&mut self, cmd: &Cmd<EngineEffect, AudioEvent>) {
            for effect in cmd.effects() {
                self.run(effect);
            }
        }

        fn run(&mut self, effect: &EngineEffect) {
            match effect {
                EngineEffect::Clear(_) | EngineEffect::Mute => {
                    self.primary = None;
                    self.preload = None;
                    self.outgoing = None;
                }
                EngineEffect::StartLoad { path, .. } => {
                    self.primary = Some(path.clone());
                    self.preload = None;
                    self.outgoing = None;
                }
                EngineEffect::StartHandover { path, .. } => {
                    self.outgoing = self.primary.replace(path.clone());
                    self.preload = None;
                }
                EngineEffect::Preload { path, .. } => {
                    self.preload = Some(path.clone());
                }
                EngineEffect::Promote(_) => {
                    self.primary = self.preload.take();
                }
                EngineEffect::DropOutgoing => {
                    self.outgoing = None;
                }
                EngineEffect::Open { .. }
                | EngineEffect::Decode(_)
                | EngineEffect::Start(_)
                | EngineEffect::Resume { .. }
                | EngineEffect::Play
                | EngineEffect::Pause
                | EngineEffect::Seek(_)
                | EngineEffect::SetGain(_)
                | EngineEffect::Arm(_)
                | EngineEffect::Crossfade { .. }
                | EngineEffect::CancelCrossfade
                | EngineEffect::Ramp { .. }
                | EngineEffect::SetSpeed(_)
                | EngineEffect::RestartGapless(_)
                | EngineEffect::Run(_)
                | EngineEffect::Report
                | EngineEffect::Advance
                | EngineEffect::Stage(_)
                | EngineEffect::Attach(_)
                | EngineEffect::TakeSignals(_) => {}
            }
        }
    }

    struct Wiring {
        model: Model,
        engine: Engine,
        sinks: Sinks,
        log: Vec<Cmd<EngineEffect, AudioEvent>>,
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

        fn engine_step(&mut self, message: EngineMessage) {
            let effect = self
                .engine
                .transition(message)
                .unwrap_or_else(|Unhandled| Cmd::none());
            self.sinks.execute(&effect);
            self.log.push(effect);
        }

        fn press(&mut self, message: Message) {
            let produced = update(&mut self.model, message, Moment::default()).unwrap();
            let audio: Vec<AudioCmd> = produced.iter().filter_map(as_audio).collect();
            for command in audio {
                self.engine_step(cmd(command));
            }
        }

        fn decoded(&mut self) {
            self.engine_step(EngineMessage::Decoded(Some(TOTAL)));
            let started = self.log.last().is_some_and(|logged| {
                matches!(logged.effects().as_slice(), [EngineEffect::Start(_)])
            });
            if started {
                self.press(Message::Audio(AudioEvent::Loaded(Some(TOTAL))));
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
                self.sinks.primary,
                Some(PathBuf::from(path)),
                "the open stream must be {path}, got {:?}",
                self.sinks.primary
            );
        }
    }

    fn load(path: &str) -> AudioCmd {
        AudioCmd::Load(kernel::TrackLoad {
            path: PathBuf::from(path),
            gain: None,
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
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::ListDevices,
        ],
        vec![
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::ListDevices,
        ]
    )]
    #[case::empty(Vec::new(), Vec::new())]
    fn coalesced_keeps_the_last_idempotent_command(
        #[case] batch: Vec<AudioCmd>,
        #[case] expected: Vec<AudioCmd>,
    ) {
        assert_eq!(keep_last_idempotent(batch), expected);
    }

    #[test]
    fn a_picked_row_fades_over_the_stream_the_media_key_started() {
        let mut wiring = Wiring::new();

        wiring.press(Message::Playback(PlaybackRequest::Play));
        wiring.decoded();
        wiring.one_open_stream("the media key's own start");

        wiring.model.workspace.browse.cursor = Cursor::with_len(3).at(2);
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
        wiring.model.workspace.browse.cursor = Cursor::with_len(3).at(2);
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
