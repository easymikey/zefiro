use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, LibraryCmd, PlaybackChange},
    domain::{Pause, Percent, Player, Preload, Revision, Track},
    message::AudioFailure,
    update::player::{
        StartOrigin,
        Transition,
        handoff_effects,
        seek_effect,
        start,
        stopped_effects,
    },
};

#[derive(Debug)]
pub struct Lookahead {
    pub preload_lead: Duration,
    pub ab_loop: Option<(Duration, Duration)>,
    pub next: Option<Arc<Track>>,
}

impl Lookahead {
    fn loop_start(&self, at: Duration) -> Option<Duration> {
        self.ab_loop.and_then(|(a, b)| (at >= b).then_some(a))
    }

    fn is_preload_due(&self, track: &Track, at: Duration) -> bool {
        let duration = track.duration().unwrap_or_default();
        !duration.is_zero() && at + self.preload_lead > duration
    }

    fn preloading(self, track: Arc<Track>, at: Duration) -> (Player, Cmd) {
        let (preload, cmd) = match self.next {
            Some(next) if self.is_preload_due(&track, at) => {
                let cmd = Cmd::Batch(vec![
                    Effect::Audio(AudioCmd::Preload {
                        path: next.path().to_path_buf(),
                        gain: next.audio_format().replay_gain,
                        revision: Revision::UNSTAMPED,
                    }),
                    Effect::Library(LibraryCmd::PrefetchCover(
                        next.path().to_path_buf(),
                    )),
                ]);
                (Preload::Queued(next), cmd)
            }
            Some(_) | None => (Preload::None, Cmd::None),
        };
        (Player::Playing { track, at, preload }, cmd)
    }
}

impl Player {
    pub(super) fn loaded(self, total: Option<Duration>) -> Transition {
        match self {
            Player::Loading { track, at } => {
                let track = match total {
                    Some(total) => Arc::new(track.with_duration(total)),
                    None => track,
                };
                Ok((
                    Player::Playing {
                        track,
                        at,
                        preload: Preload::None,
                    },
                    Cmd::None,
                ))
            }
            other @ (Player::Playing { .. }
            | Player::Paused { .. }
            | Player::Stopped) => other.refuse(),
        }
    }

    pub(super) fn failed(self, failure: &AudioFailure) -> (Player, Cmd) {
        match failure {
            AudioFailure::OutputLost { .. } => self.output_lost(),
            AudioFailure::Decode { .. }
            | AudioFailure::Device { .. }
            | AudioFailure::Stream { .. }
            | AudioFailure::Preload { .. } => self.load_failed(),
            AudioFailure::Seek { .. } => (self, Cmd::None),
        }
    }

    fn output_lost(self) -> (Player, Cmd) {
        match self {
            Player::Playing { track, at, .. } => (
                Player::Paused {
                    track,
                    at,
                    pause: Pause::ByListener,
                },
                PlaybackChange::Pause.cued(),
            ),
            Player::Loading { .. } => (Player::Stopped, stopped_effects()),
            other @ (Player::Paused { .. } | Player::Stopped) => (other, Cmd::None),
        }
    }

    fn load_failed(self) -> (Player, Cmd) {
        match self {
            Player::Loading { .. } => (Player::Stopped, stopped_effects()),
            other @ (Player::Playing { .. }
            | Player::Paused { .. }
            | Player::Stopped) => (other, Cmd::None),
        }
    }

    pub(super) fn positioned(self, at: Duration, lookahead: Lookahead) -> Transition {
        match (self, lookahead.loop_start(at)) {
            (Player::Playing { track, preload, .. }, Some(a)) => Ok((
                Player::Playing {
                    track,
                    at: a,
                    preload: preload.seek_reset(),
                },
                seek_effect(a),
            )),
            (
                Player::Playing {
                    track,
                    preload: Preload::None,
                    ..
                },
                None,
            ) => Ok(lookahead.preloading(track, at)),
            (Player::Playing { track, preload, .. }, None) => {
                Ok((Player::Playing { track, at, preload }, Cmd::None))
            }
            (Player::Paused { track, pause, .. }, Some(a)) => Ok((
                Player::Paused {
                    track,
                    at: a,
                    pause,
                },
                seek_effect(a),
            )),
            (paused @ Player::Paused { .. }, None) => paused.refuse(),
            (other @ (Player::Loading { .. } | Player::Stopped), Some(_) | None) => {
                other.refuse()
            }
        }
    }

    pub(super) fn track_changed(self, next: Option<Arc<Track>>) -> Transition {
        match self {
            Player::Playing { track, .. } => {
                let track = next.unwrap_or(track);
                let effects = handoff_effects(&track, PlaybackChange::Play);
                Ok((
                    Player::Playing {
                        track,
                        at: Duration::ZERO,
                        preload: Preload::None,
                    },
                    effects,
                ))
            }
            Player::Paused { track, pause, .. } => {
                let track = next.unwrap_or(track);
                let effects = handoff_effects(&track, PlaybackChange::Pause);
                Ok((
                    Player::Paused {
                        track,
                        at: Duration::ZERO,
                        pause,
                    },
                    effects,
                ))
            }
            other @ (Player::Loading { .. } | Player::Stopped) => other.refuse(),
        }
    }

    pub(super) fn ended(self, next: Option<Arc<Track>>, volume: Percent) -> Transition {
        match self {
            Player::Playing { .. } => Ok(next.map_or_else(
                || (Player::Stopped, stopped_effects()),
                |track| start(track, volume, StartOrigin::TrackEnded),
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => other.refuse(),
        }
    }
}
