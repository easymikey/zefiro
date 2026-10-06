pub mod events;
mod requests;
pub mod stamp;

use std::{mem, sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, TrackLoad},
    domain::{
        cue::PlaybackChange,
        player::{AbLoop, PausedBy, Player},
        playlist::Playlist,
        revision::Revisions,
        settings::Settings,
        time::Moment,
        track::{Track, TrackRef},
        transport::Transport,
        workspace::Workspace,
    },
    message::{AudioError, Timer},
    update::{
        machine::{Machine, Unhandled},
        player::{
            events::{Lookahead, handover_effects, next_decision},
            stamp::{Anchor, Stamp, StartOrigin},
        },
        successor::successor,
    },
};

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
    OutputLost(Moment),
    Start {
        track: Arc<Track>,
        stamp: Stamp,
    },
    Loaded {
        total: Option<Duration>,
        anchor: Anchor,
    },
    Error(AudioError),
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
    SpeedChanged(Anchor),
}

impl Machine for Player {
    type Message = PlayerMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: PlayerMessage) -> Result<Cmd, Unhandled> {
        match message {
            PlayerMessage::Toggle { current, stamp } => self.toggle(current, stamp),
            PlayerMessage::OutputLost(now) => self.output_lost(now),
            PlayerMessage::Stop => Ok(self.stop()),
            PlayerMessage::Hold(now) => self.pause(now, PausedBy::Overlay),
            PlayerMessage::Release(anchor) => self.release(anchor),
            PlayerMessage::Seek { target, now } => self.seek(target, now),
            PlayerMessage::SleepFired(now) => match self {
                Player::Playing { .. } => self.pause(now, PausedBy::Listener),
                Player::Loading(..) | Player::Paused { .. } | Player::Stopped => {
                    Ok(Cmd::none())
                }
            },
            PlayerMessage::Start { track, stamp } => {
                Ok(self.start(track, StartOrigin::User(stamp)))
            }
            PlayerMessage::Loaded { total, anchor } => self.loaded(total, anchor),
            PlayerMessage::Error(error) => self.failed(&error),
            PlayerMessage::SpeedChanged(anchor) => {
                let current = mem::replace(self, Player::Stopped);
                *self = current.reanchored(anchor.since, anchor.speed);
                Ok(Cmd::none())
            }
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
        PlaybackChange::Stop
            .cued()
            .then(Cmd::from(Effect::Macos(MacosCmd::NowPlaying(None))))
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
        *self = Player::Loading(track);
        cmd
    }
}

pub(crate) fn update_player(
    playback: &mut PlaybackParts<'_>,
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let seeks = matches!(message, PlayerMessage::Seek { .. });
    let candidate = playback.revisions.effects.next();
    let cmd = playback.player.transition(message)?;
    playback.revisions.effects = candidate;
    let armed = if seeks {
        timer(playback, now)
    } else {
        arm(playback, now)
    };
    Ok(cmd.then(armed))
}

pub(crate) fn record(
    playback_parts: &mut PlaybackParts<'_>,
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let before = playback_parts.player.clone();
    let cmd = playback_parts.player.transition(message)?;
    if *playback_parts.player == before {
        return Ok(cmd);
    }
    playback_parts.revisions.effects = playback_parts.revisions.effects.next();
    Ok(cmd.then(arm(playback_parts, now)))
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
        ab_loop,
        next: successor(playback.playlist, playback.queue)
            .track()
            .cloned(),
        duration: duration_of(playback.player),
        now,
        revision: playback.revisions.effects.next(),
        cover_side: crate::update::cover_side(playback.workspace, playback.settings),
    }
}

pub(crate) fn arm(playback: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { playhead, .. } = &*playback.player else {
        return Cmd::none();
    };
    Cmd::from(Effect::Macos(MacosCmd::SetPosition(
        playhead.position_at(now),
    )))
    .then(timer(playback, now))
}

fn timer(playback: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { playhead, .. } = &*playback.player else {
        return Cmd::none();
    };
    next_decision(*playhead, &lookahead(playback, now)).map_or(Cmd::none(), |delay| {
        Effect::After {
            delay,
            timer: Timer::Lookahead(playback.revisions.issue_lookahead()),
        }
        .into()
    })
}

pub(crate) fn stopped_effects() -> Cmd {
    PlaybackChange::Stop
        .effects()
        .into_iter()
        .chain([Effect::Macos(MacosCmd::NowPlaying(None))])
        .collect()
}
