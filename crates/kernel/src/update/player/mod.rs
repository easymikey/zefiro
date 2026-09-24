mod events;
mod requests;

use std::{sync::Arc, time::Duration};

pub use events::Lookahead;

use crate::{
    cmd::{
        AudioCmd,
        Cmd,
        Cue,
        Effect,
        LibraryCmd,
        NowPlaying,
        PlaybackChange,
        SystemCmd,
    },
    domain::{Percent, Player, Revision, Track},
    message::AudioFailure,
    update::machine::{Machine, Rejected},
};

#[derive(Debug)]
pub enum PlayerMessage {
    Toggle {
        current: Option<Arc<Track>>,
        volume: Percent,
    },
    Stop,
    Hold,
    Release,
    Seek(Duration),
    SleepFired,
    Start {
        track: Arc<Track>,
        volume: Percent,
    },
    Loaded {
        total: Option<Duration>,
    },
    Error(AudioFailure),
    Position {
        at: Duration,
        lookahead: Lookahead,
    },
    TrackChanged {
        next: Option<Arc<Track>>,
    },
    Ended {
        next: Option<Arc<Track>>,
        volume: Percent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRejection {
    Stopped,
    Loading,
    Playing,
    Paused,
}

type Transition = Result<(Player, Cmd), Rejected<Player>>;

impl Machine for Player {
    type Message = PlayerMessage;
    type Rejection = PlayerRejection;
    type Effect = Cmd;

    fn transition(self, message: PlayerMessage) -> Transition {
        match message {
            PlayerMessage::Toggle { current, volume } => self.toggle(current, volume),
            PlayerMessage::Stop => Ok((Player::Stopped, stopped_effects())),
            PlayerMessage::Hold => self.hold(),
            PlayerMessage::Release => self.release(),
            PlayerMessage::Seek(target) => self.seek(target),
            PlayerMessage::SleepFired => self.sleep_fired(),
            PlayerMessage::Start { track, volume } => {
                Ok(start(track, volume, StartOrigin::User))
            }
            PlayerMessage::Loaded { total } => self.loaded(total),
            PlayerMessage::Error(failure) => Ok(self.failed(&failure)),
            PlayerMessage::Position { at, lookahead } => self.positioned(at, lookahead),
            PlayerMessage::TrackChanged { next } => self.track_changed(next),
            PlayerMessage::Ended { next, volume } => self.ended(next, volume),
        }
    }
}

impl Player {
    fn refuse(self) -> Transition {
        let reason = match &self {
            Player::Stopped => PlayerRejection::Stopped,
            Player::Loading { .. } => PlayerRejection::Loading,
            Player::Playing { .. } => PlayerRejection::Playing,
            Player::Paused { .. } => PlayerRejection::Paused,
        };
        Err(Rejected {
            state: self,
            reason,
        })
    }
}

#[derive(Clone, Copy)]
enum StartOrigin {
    User,
    TrackEnded,
}

fn start(track: Arc<Track>, volume: Percent, origin: StartOrigin) -> (Player, Cmd) {
    let path = track.path().to_path_buf();
    let gain = track.audio_format().replay_gain;
    let mut effects = match origin {
        StartOrigin::User => vec![Effect::Audio(AudioCmd::Stop)],
        StartOrigin::TrackEnded => Vec::new(),
    };
    effects.extend([
        Effect::Audio(AudioCmd::Load {
            path,
            gain,
            revision: Revision::UNSTAMPED,
        }),
        Effect::Audio(AudioCmd::Volume(volume)),
        Effect::Library(LibraryCmd::AppendHistory {
            track: Arc::clone(&track),
            revision: Revision::UNSTAMPED,
        }),
        Effect::System(SystemCmd::NowPlaying(now_playing(&track))),
    ]);
    effects.extend(PlaybackChange::Play.effects());
    effects.extend([
        Effect::Animate(Cue::TrackChanged),
        Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Play)),
    ]);
    (
        Player::Loading {
            track,
            at: Duration::ZERO,
        },
        Cmd::Batch(effects),
    )
}

fn seek_effect(target: Duration) -> Cmd {
    Cmd::Batch(vec![
        Effect::Audio(AudioCmd::Seek(target)),
        Effect::System(SystemCmd::PlaybackPosition(target)),
    ])
}

fn handoff_effects(track: &Arc<Track>, playback: PlaybackChange) -> Cmd {
    let mut effects = vec![
        Effect::Library(LibraryCmd::AppendHistory {
            track: Arc::clone(track),
            revision: Revision::UNSTAMPED,
        }),
        Effect::System(SystemCmd::NowPlaying(now_playing(track))),
    ];
    effects.extend(playback.effects());
    effects.extend([
        Effect::Animate(Cue::TrackChanged),
        Effect::Animate(Cue::PlaybackChanged(playback)),
    ]);
    Cmd::Batch(effects)
}

pub(super) fn stopped_effects() -> Cmd {
    let mut effects = PlaybackChange::Stop.effects().to_vec();
    effects.extend([
        Effect::System(SystemCmd::NowPlaying(NowPlaying::default())),
        Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Stop)),
    ]);
    Cmd::Batch(effects)
}

fn now_playing(track: &Track) -> NowPlaying {
    NowPlaying::Track {
        title: track.song_title(),
        artist: track.tags().artist.clone(),
        album: track.tags().album.clone(),
        duration: track.duration().unwrap_or_default(),
        path: track.path().to_path_buf(),
    }
}
