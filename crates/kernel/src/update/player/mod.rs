mod events;
mod requests;

use std::{sync::Arc, time::Duration};

pub use events::Lookahead;
use events::next_decision;

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
    domain::{
        AbLoop,
        Model,
        Moment,
        Player,
        PlaylistIndex,
        Revision,
        Speed,
        Track,
        UnixSeconds,
        Workspace,
        playlist::{self, Playlist, RepeatMode},
    },
    message::{AudioFailure, Timer},
    update::{
        machine::{Machine, Rejected},
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub now: Moment,
    pub speed: Speed,
}

#[derive(Debug, Clone, Copy)]
pub struct Resume {
    pub anchor: Anchor,
}

#[derive(Debug)]
pub enum PlayerMessage {
    Toggle {
        current: Option<Arc<Track>>,
        resume: Resume,
    },
    Stop,
    Hold(Moment),
    Release(Anchor),
    Seek {
        target: Duration,
        now: Moment,
    },
    SleepFired(Moment),
    Start {
        track: Arc<Track>,
    },
    Loaded {
        total: Option<Duration>,
        anchor: Anchor,
    },
    Error {
        failure: AudioFailure,
        now: Moment,
    },
    Playhead {
        offset: Duration,
        lookahead: Lookahead,
    },
    Reported {
        offset: Duration,
        now: Moment,
    },
    TrackChanged {
        next: Option<Arc<Track>>,
        now: Moment,
    },
    Ended {
        next: Option<Arc<Track>>,
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
            PlayerMessage::Toggle { current, resume } => self.toggle(current, resume),
            PlayerMessage::Stop => Ok((Player::Stopped, stopped_effects())),
            PlayerMessage::Hold(now) => self.hold(now),
            PlayerMessage::Release(anchor) => self.release(anchor),
            PlayerMessage::Seek { target, now } => self.seek(target, now),
            PlayerMessage::SleepFired(now) => self.sleep_fired(now),
            PlayerMessage::Start { track } => Ok(start(track, StartOrigin::User)),
            PlayerMessage::Loaded { total, anchor } => self.loaded(total, anchor),
            PlayerMessage::Error { failure, now } => Ok(self.failed(&failure, now)),
            PlayerMessage::Playhead { offset, lookahead } => {
                self.positioned(offset, lookahead)
            }
            PlayerMessage::Reported { offset, now } => self.reported(offset, now),
            PlayerMessage::TrackChanged { next, now } => self.track_changed(next, now),
            PlayerMessage::Ended { next } => self.ended(next),
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

pub(crate) fn account(
    model: &mut Model,
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let since = playing_since(&model.player);
    let cmd = model.player.update(message)?;
    accumulate(&mut model.workspace, since, now);
    let position = position_report(&cmd);
    Ok(cmd.then(arm(model, now, position)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PositionReport {
    AlreadySent,
    Pending,
}

fn position_report(cmd: &Cmd) -> PositionReport {
    let sent = cmd
        .effects()
        .any(|effect| matches!(effect, Effect::System(SystemCmd::PlaybackPosition(_))));
    if sent {
        PositionReport::AlreadySent
    } else {
        PositionReport::Pending
    }
}

pub(crate) fn lookahead(model: &Model, now: Moment) -> Lookahead {
    let ab_loop = match model.transport.ab {
        Some(AbLoop::Full { a, b }) => Some((a, b)),
        Some(AbLoop::AOnly(_)) | None => None,
    };
    let duration = model.player.current().map_or(Duration::ZERO, |track| {
        track.duration().unwrap_or(Duration::ZERO)
    });
    Lookahead {
        preload_lead: model.transport.preload_lead,
        ab_loop,
        next: next_track(&model.playlist, &model.queue),
        duration,
        now,
    }
}

fn next_track(playlist: &Playlist, queue: &[PlaylistIndex]) -> Option<Arc<Track>> {
    if matches!(playlist.repeat, RepeatMode::One) {
        return playlist.current().cloned();
    }
    if let Some(index) = queue.first() {
        return playlist.tracks.get(index.get()).cloned();
    }
    playlist::upcoming(playlist).cloned()
}

pub(crate) fn arm(model: &Model, now: Moment, position: PositionReport) -> Cmd {
    let Player::Playing { head, .. } = &model.player else {
        return Cmd::None;
    };
    let reported = match position {
        PositionReport::Pending => {
            Effect::System(SystemCmd::PlaybackPosition(head.position_at(now))).into()
        }
        PositionReport::AlreadySent => Cmd::None,
    };
    let look = lookahead(model, now);
    let timer = next_decision(*head, &look, now).map_or(Cmd::None, |delay| {
        Effect::After {
            delay,
            message: Timer::Mark(Revision::UNSTAMPED),
        }
        .into()
    });
    reported.then(timer)
}

pub(crate) fn playing_since(player: &Player) -> Option<Moment> {
    match player {
        Player::Playing { head, .. } => Some(head.since),
        Player::Stopped | Player::Loading { .. } | Player::Paused { .. } => None,
    }
}

pub(crate) fn accumulate(
    workspace: &mut Workspace,
    since: Option<Moment>,
    now: Moment,
) {
    if let Some(since) = since {
        workspace.played_for += now.elapsed_since(since);
    }
}

#[derive(Clone, Copy)]
enum StartOrigin {
    User,
    TrackEnded,
}

fn start(track: Arc<Track>, origin: StartOrigin) -> (Player, Cmd) {
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
        Effect::Library(LibraryCmd::AppendHistory {
            track: Arc::clone(&track),
            at: UnixSeconds::UNSTAMPED,
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
            at: UnixSeconds::UNSTAMPED,
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

pub(crate) fn stopped_effects() -> Cmd {
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
