use kernel::{
    EngineRejection,
    update::{Machine, Rejected},
};

use crate::engine::{
    effect::{EngineEffect, EngineMessage},
    state::{Engine, Transition},
};

impl Machine for Engine {
    type Message = EngineMessage;
    type Rejection = EngineRejection;
    type Effect = EngineEffect;

    fn transition(
        self,
        message: EngineMessage,
    ) -> Result<(Self, EngineEffect), Rejected<Self>> {
        let moved = match (self, message) {
            (Engine::Muted(muted), message) => muted.transition(message),
            (Engine::Live(live), EngineMessage::Cmd(cmd)) => live.command(cmd),
            (Engine::Live(live), EngineMessage::Opened(outcome)) => {
                Transition::from(live.opened(outcome))
            }
            (Engine::Live(live), EngineMessage::Decoded(outcome)) => {
                Transition::from(live.decoded(outcome))
            }
            (Engine::Live(live), EngineMessage::Preloaded(outcome)) => {
                Transition::from(live.preloaded(outcome))
            }
            (Engine::Live(live), EngineMessage::Failed(fault)) => {
                Transition::from(live.failed(fault))
            }
            (Engine::Live(live), EngineMessage::Retiring { from }) => {
                Transition::from(live.retiring(from))
            }
            (Engine::Live(live), EngineMessage::DevicesListed(result)) => {
                Transition::from(live.devices_listed(result))
            }
            (Engine::Live(live), EngineMessage::Finished(slot)) => {
                Transition::from(live.finished(slot))
            }
            (Engine::Live(live), EngineMessage::Cued) => Transition::from(live.cued()),
            (Engine::Live(live), EngineMessage::Ramped(slot)) => {
                Transition::from(live.ramped(slot))
            }
        };
        match moved {
            Transition::Next(engine, io) => Ok((engine, io)),
            Transition::Rejected(rejected) => Err(rejected),
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
        Effect,
        Message,
        Model,
        Moment,
        PlaybackRequest,
        Playlist,
        Track,
        domain::{AudioFormat, Crossfade, Cursor, Tags},
        update::{Machine, update},
    };

    use crate::{
        EngineConfig,
        engine::{
            effect::{EngineEffect, EngineMessage, Slot},
            state::{Engine, Live, fixtures::config},
        },
    };

    const TOTAL: Duration = Duration::from_secs(100);
    const CROSSFADE: Duration = Duration::from_secs(7);

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
                at: Cursor::new(3),
                ..Playlist::default()
            },
            ..Model::default()
        }
    }

    fn engine_with_crossfade() -> Engine {
        Engine::Live(Live::new(EngineConfig {
            crossfade: Crossfade::clamped(CROSSFADE),
            ..config()
        }))
    }

    fn as_audio(effect: &Effect) -> Option<AudioCmd> {
        match effect {
            Effect::Audio(audio) => Some(audio.clone()),
            Effect::Library(_)
            | Effect::System(_)
            | Effect::Config(_)
            | Effect::Animate(_)
            | Effect::RollShuffle { .. }
            | Effect::WindowColors(_)
            | Effect::Setting { .. }
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

        fn apply(&mut self, io: &EngineEffect) {
            match io {
                EngineEffect::Many(steps) => {
                    for step in steps {
                        self.apply(step);
                    }
                }
                EngineEffect::Clear | EngineEffect::Mute(_) => {
                    self.primary = None;
                    self.preload = None;
                    self.outgoing = None;
                }
                EngineEffect::StartLoad { path, .. } => {
                    self.primary = Some(path.clone());
                    self.preload = None;
                    self.outgoing = None;
                }
                EngineEffect::StartFade { path, .. } => {
                    self.outgoing = self.primary.replace(path.clone());
                    self.preload = None;
                }
                EngineEffect::PreloadCrossfade { path, .. }
                | EngineEffect::PreloadGapless(path) => {
                    self.preload = Some(path.clone());
                }
                EngineEffect::Promote { .. } => {
                    self.primary = self.preload.take();
                }
                EngineEffect::DropOutgoing => {
                    self.outgoing = None;
                }
                EngineEffect::Nothing
                | EngineEffect::Send(_)
                | EngineEffect::Open { .. }
                | EngineEffect::Decode(_)
                | EngineEffect::Start { .. }
                | EngineEffect::Resume { .. }
                | EngineEffect::Play
                | EngineEffect::Pause
                | EngineEffect::Seek(_)
                | EngineEffect::SetVolume(_)
                | EngineEffect::Arm { .. }
                | EngineEffect::Crossfade { .. }
                | EngineEffect::Unfade
                | EngineEffect::Ramp { .. }
                | EngineEffect::SetSpeed(_)
                | EngineEffect::RestartGapless(_)
                | EngineEffect::ListDevices
                | EngineEffect::Report
                | EngineEffect::Advance => {}
            }
        }
    }

    struct Wiring {
        model: Model,
        engine: Engine,
        sinks: Sinks,
        log: Vec<EngineEffect>,
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
            let io = self.engine.update(message).unwrap();
            self.sinks.apply(&io);
            self.log.push(io);
        }

        fn press(&mut self, message: Message) {
            let cmd = update(&mut self.model, message, Moment::default()).unwrap();
            let audio: Vec<AudioCmd> = cmd.effects().filter_map(as_audio).collect();
            for command in audio {
                self.engine_step(EngineMessage::Cmd(command));
            }
        }

        fn decoded(&mut self) {
            self.engine_step(EngineMessage::Decoded(Ok(Some(TOTAL))));
            if matches!(self.log.last(), Some(EngineEffect::Start { .. })) {
                self.press(Message::Audio(AudioEvent::Loaded { total: Some(TOTAL) }));
            }
        }

        fn handover_settles(&mut self) {
            self.engine_step(EngineMessage::Ramped(Slot::Outgoing));
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
    fn three_skips_inside_one_fade_end_on_a_single_stream() {
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
