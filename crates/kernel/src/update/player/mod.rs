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
        TrackLoad,
    },
    domain::{
        AbLoop,
        HistoryEntry,
        Moment,
        PRELOAD_LEAD,
        PausedBy,
        Player,
        Revision,
        Revisions,
        Settings,
        Speed,
        Track,
        TrackRef,
        Transport,
        Workspace,
        playlist::Playlist,
    },
    message::{AudioError, Timer},
    update::{
        audio,
        machine::{Machine, Unhandled},
    },
};

#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub since: Moment,
    pub speed: Speed,
}

impl Anchor {
    pub(crate) fn at(transport: &Transport, now: Moment) -> Self {
        Anchor {
            since: now,
            speed: transport.speed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub anchor: Anchor,
    pub revision: Revision,
}

impl Stamp {
    pub(crate) fn pending(
        transport: &Transport,
        revisions: &Revisions,
        now: Moment,
    ) -> Self {
        Stamp {
            anchor: Anchor::at(transport, now),
            revision: revisions.effects.next(),
        }
    }
}

pub(crate) struct PlaybackParts<'a> {
    pub(crate) player: &'a mut Player,
    pub(crate) transport: &'a mut Transport,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) queue: &'a mut Vec<TrackRef>,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) settings: &'a mut Settings,
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
    LookaheadReached {
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

impl Machine for Player {
    type Message = PlayerMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: PlayerMessage) -> Result<Cmd, Unhandled> {
        match message {
            PlayerMessage::Toggle { current, stamp } => self.toggle(current, stamp),
            PlayerMessage::Stop => Ok(self.stop()),
            PlayerMessage::Hold(now) => Ok(self.pause(now, PausedBy::Overlay)),
            PlayerMessage::Release(anchor) => Ok(self.release(anchor)),
            PlayerMessage::Seek { target, now } => self.seek(target, now),
            PlayerMessage::SleepFired(now) => Ok(self.pause(now, PausedBy::Listener)),
            PlayerMessage::Start { track, stamp } => {
                Ok(self.start(track, StartOrigin::User(stamp)))
            }
            PlayerMessage::Loaded { total, anchor } => self.loaded(total, anchor),
            PlayerMessage::Error { error, now } => Ok(self.failed(&error, now)),
            PlayerMessage::LookaheadReached { offset, lookahead } => {
                self.positioned(offset, lookahead)
            }
            PlayerMessage::Playhead { offset, now } => self.reported(offset, now),
            PlayerMessage::TrackChanged { next, now } => self.track_changed(next, now),
            PlayerMessage::Ended { next, stamp } => self.ended(next, stamp),
        }
    }
}

impl Player {
    fn stop(&mut self) -> Cmd {
        *self = Player::Stopped;
        stopped_effects()
    }

    fn start(&mut self, track: Arc<Track>, origin: StartOrigin) -> Cmd {
        let request = TrackLoad::for_track(&track, origin.stamp().revision);
        let load = Effect::Audio(AudioCmd::Load(request));
        let cmd = origin
            .stop()
            .into_iter()
            .chain([load])
            .chain(handover_effects(
                &track,
                PlaybackChange::Play,
                origin.stamp().anchor.since,
            ))
            .collect();
        *self = Player::Loading {
            track,
            at: Duration::ZERO,
        };
        cmd
    }
}

pub(crate) fn update_player(
    playback: &mut PlaybackParts<'_>,
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let since = playing_since(playback.player);
    let candidate = playback.revisions.effects.next();
    let cmd = playback.player.transition(message)?;
    commit(playback.revisions, candidate, &cmd);
    if let Some(since) = since {
        accumulate(playback.workspace, since, now);
    }
    let position_sent = cmd
        .effects()
        .any(|effect| matches!(effect, Effect::Macos(MacosCmd::SetPosition(_))));
    let armed = if position_sent {
        timer(playback, now)
    } else {
        arm(playback, now)
    };
    Ok(cmd.then(armed))
}

pub(crate) fn commit(revisions: &mut Revisions, candidate: Revision, cmd: &Cmd) {
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

pub(crate) fn duration_of(player: &Player) -> Duration {
    player
        .current()
        .and_then(|track| track.duration())
        .unwrap_or(Duration::ZERO)
}

pub(crate) fn lookahead(playback: &PlaybackParts<'_>, now: Moment) -> Lookahead {
    let ab_loop = match playback.transport.ab_loop {
        Some(AbLoop::Full { a, b }) => Some((a, b)),
        Some(AbLoop::AOnly(_)) | None => None,
    };
    Lookahead {
        preload_lead: PRELOAD_LEAD,
        ab_loop,
        next: audio::successor(playback.playlist, playback.queue)
            .track()
            .cloned(),
        duration: duration_of(playback.player),
        now,
        revision: playback.revisions.effects.next(),
    }
}

pub(crate) fn arm(playback: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { head, .. } = &*playback.player else {
        return Cmd::none();
    };
    Cmd::from(Effect::Macos(MacosCmd::SetPosition(head.position_at(now))))
        .then(timer(playback, now))
}

fn timer(playback: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { head, .. } = &*playback.player else {
        return Cmd::none();
    };
    next_decision(*head, &lookahead(playback, now)).map_or(Cmd::none(), |delay| {
        Effect::After {
            delay,
            timer: Timer::Lookahead(playback.revisions.issue_lookahead()),
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
    User(Stamp),
    TrackEnded(Stamp),
}

impl StartOrigin {
    fn stamp(self) -> Stamp {
        match self {
            StartOrigin::User(stamp) | StartOrigin::TrackEnded(stamp) => stamp,
        }
    }

    fn stop(self) -> Option<Effect> {
        match self {
            StartOrigin::User(_) => Some(Effect::Audio(AudioCmd::Stop)),
            StartOrigin::TrackEnded(_) => None,
        }
    }
}

fn seek_effect(target: Duration) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Seek(target)),
        Effect::Macos(MacosCmd::SetPosition(target)),
    ])
}

fn handover_effects(
    track: &Arc<Track>,
    playback: PlaybackChange,
    now: Moment,
) -> Vec<Effect> {
    [
        Effect::Library(LibraryCmd::AppendHistory(HistoryEntry::from_track(
            track, now,
        ))),
        Effect::Macos(MacosCmd::NowPlaying(Some(Arc::clone(track)))),
    ]
    .into_iter()
    .chain(playback.effects())
    .chain([
        Effect::Animate(Cue::TrackChanged),
        Effect::Animate(Cue::PlaybackChanged(playback)),
    ])
    .collect()
}

pub(crate) fn stopped_effects() -> Cmd {
    PlaybackChange::Stop
        .effects()
        .into_iter()
        .chain([
            Effect::Macos(MacosCmd::NowPlaying(None)),
            Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Stop)),
        ])
        .collect()
}
