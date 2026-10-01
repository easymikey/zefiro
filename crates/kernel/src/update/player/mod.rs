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
        MacosCmd,
        PlaybackChange,
        TrackRequest,
    },
    domain::{
        AbLoop,
        HistoryEntry,
        Model,
        Moment,
        PRELOAD_LEAD,
        Pause,
        Player,
        Revision,
        Revisions,
        Speed,
        Track,
        Workspace,
    },
    message::{AudioError, Timer},
    update::{
        audio,
        error::UpdateError,
        machine::{Machine, Rejected},
    },
};

#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub since: Moment,
    pub speed: Speed,
}

impl Anchor {
    pub(crate) fn at(model: &Model, now: Moment) -> Self {
        Anchor {
            since: now,
            speed: model.transport.speed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub anchor: Anchor,
    pub revision: Revision,
}

impl Stamp {
    pub(crate) fn issue(model: &Model, now: Moment) -> Self {
        Stamp {
            anchor: Anchor::at(model, now),
            revision: model.revisions.effects.next(),
        }
    }
}

#[derive(Debug)]
pub enum PlayerMessage {
    Toggle {
        current: Option<Arc<Track>>,
        stamp: Stamp,
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
        stamp: Stamp,
    },
    Loaded {
        total: Option<Duration>,
        anchor: Anchor,
    },
    Error {
        error: AudioError,
        now: Moment,
    },
    MarkReached {
        offset: Duration,
        lookahead: Lookahead,
    },
    Playhead {
        offset: Duration,
        now: Moment,
    },
    TrackChanged {
        next: Option<Arc<Track>>,
        now: Moment,
    },
    Ended {
        next: Option<Arc<Track>>,
        stamp: Stamp,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerError {
    Stopped,
    Loading,
    Playing,
    Paused,
}

type Transition = Result<(Player, Cmd), Rejected<Player>>;

impl Machine for Player {
    type Message = PlayerMessage;
    type Error = PlayerError;
    type Effect = Cmd;

    fn transition(self, message: PlayerMessage) -> Transition {
        match message {
            PlayerMessage::Toggle { current, stamp } => self.toggle(current, stamp),
            PlayerMessage::Stop => Ok((Player::Stopped, stopped_effects())),
            PlayerMessage::Hold(now) => self.pause(now, Pause::ByOverlay),
            PlayerMessage::Release(anchor) => self.release(anchor),
            PlayerMessage::Seek { target, now } => self.seek(target, now),
            PlayerMessage::SleepFired(now) => self.pause(now, Pause::ByListener),
            PlayerMessage::Start { track, stamp } => {
                Ok(start(track, StartOrigin::User, stamp))
            }
            PlayerMessage::Loaded { total, anchor } => self.loaded(total, anchor),
            PlayerMessage::Error { error, now } => Ok(self.failed(&error, now)),
            PlayerMessage::MarkReached { offset, lookahead } => {
                self.positioned(offset, lookahead)
            }
            PlayerMessage::Playhead { offset, now } => self.reported(offset, now),
            PlayerMessage::TrackChanged { next, now } => self.track_changed(next, now),
            PlayerMessage::Ended { next, stamp } => self.ended(next, stamp),
        }
    }
}

impl Player {
    fn refuse(self) -> Transition {
        let reason = match &self {
            Player::Stopped => PlayerError::Stopped,
            Player::Loading { .. } => PlayerError::Loading,
            Player::Playing { .. } => PlayerError::Playing,
            Player::Paused { .. } => PlayerError::Paused,
        };
        Err(Rejected {
            state: self,
            reason,
        })
    }
}

pub(crate) fn update_player(
    model: &mut Model,
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let since = playing_since(&model.player);
    let candidate = model.revisions.effects.next();
    let cmd = model.player.update(message)?;
    committed(&mut model.revisions, candidate, &cmd);
    if let Some(since) = since {
        accumulate(&mut model.workspace, since, now);
    }
    let position_sent = cmd
        .effects()
        .any(|effect| matches!(effect, Effect::Macos(MacosCmd::PlaybackPosition(_))));
    let armed = if position_sent {
        timer(model, now)
    } else {
        arm(model, now)
    };
    Ok(cmd.then(armed))
}

pub(crate) fn committed(revisions: &mut Revisions, candidate: Revision, cmd: &Cmd) {
    let loads = cmd.effects().any(|effect| {
        matches!(
            effect,
            Effect::Audio(AudioCmd::Load(_) | AudioCmd::Preload(_))
        )
    });
    if loads {
        revisions.effects = candidate;
    }
}

pub(crate) fn duration_of(model: &Model) -> Duration {
    model
        .player
        .current()
        .and_then(|track| track.duration())
        .unwrap_or_default()
}

pub(crate) fn lookahead(model: &Model, now: Moment) -> Lookahead {
    let ab_loop = match model.transport.ab {
        Some(AbLoop::Full { a, b }) => Some((a, b)),
        Some(AbLoop::AOnly(_)) | None => None,
    };
    Lookahead {
        preload_lead: PRELOAD_LEAD,
        ab_loop,
        next: audio::successor(&model.playlist, &model.queue)
            .track()
            .cloned(),
        duration: duration_of(model),
        now,
        revision: model.revisions.effects.next(),
    }
}

pub(crate) fn arm(model: &mut Model, now: Moment) -> Cmd {
    let Player::Playing { head, .. } = &model.player else {
        return Cmd::None;
    };
    Cmd::from(Effect::Macos(MacosCmd::PlaybackPosition(
        head.position_at(now),
    )))
    .then(timer(model, now))
}

fn timer(model: &mut Model, now: Moment) -> Cmd {
    let Player::Playing { head, .. } = &model.player else {
        return Cmd::None;
    };
    next_decision(*head, &lookahead(model, now)).map_or(Cmd::None, |delay| {
        Effect::After {
            delay,
            message: Timer::Mark(model.revisions.issue_mark()),
        }
        .into()
    })
}

pub(crate) fn playing_since(player: &Player) -> Option<Moment> {
    match player {
        Player::Playing { head, .. } => Some(head.since),
        Player::Stopped | Player::Loading { .. } | Player::Paused { .. } => None,
    }
}

pub(crate) fn accumulate(workspace: &mut Workspace, since: Moment, now: Moment) {
    workspace.played_for += now.elapsed_since(since);
}

#[derive(Clone, Copy)]
enum StartOrigin {
    User,
    TrackEnded,
}

impl StartOrigin {
    fn stop(self) -> Option<Effect> {
        match self {
            StartOrigin::User => Some(Effect::Audio(AudioCmd::Stop)),
            StartOrigin::TrackEnded => None,
        }
    }
}

fn start(track: Arc<Track>, origin: StartOrigin, stamp: Stamp) -> (Player, Cmd) {
    let request = TrackRequest::for_track(&track, stamp.revision);
    let load = Effect::Audio(AudioCmd::Load(request));
    let effects = origin
        .stop()
        .into_iter()
        .chain([load])
        .chain(handover_effects(
            &track,
            PlaybackChange::Play,
            stamp.anchor.since,
        ))
        .collect();
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
        Effect::Macos(MacosCmd::PlaybackPosition(target)),
    ])
}

fn handover_effects(track: &Arc<Track>, playback: PlaybackChange, now: Moment) -> Cmd {
    let mut effects = vec![
        Effect::Library(LibraryCmd::AppendHistory(HistoryEntry::from_track(
            track, now,
        ))),
        Effect::Macos(MacosCmd::NowPlaying(Some(Arc::clone(track)))),
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
        Effect::Macos(MacosCmd::NowPlaying(None)),
        Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Stop)),
    ]);
    Cmd::Batch(effects)
}
