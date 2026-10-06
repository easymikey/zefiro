pub mod events;
mod requests;
pub mod stamp;

use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, TrackLoad},
    domain::{
        cue::PlaybackChange,
        player::{AbLoop, PausedBy, Player},
        playhead::Playhead,
        playlist::Playlist,
        revision::Revisions,
        settings::Settings,
        time::Moment,
        track::{Track, TrackSource},
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
    pub(crate) queue: &'a mut Vec<TrackSource>,
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
        duration: Option<Duration>,
        anchor: Anchor,
    },
    Error(AudioError),
    LookaheadReached {
        position: Duration,
        lookahead: Lookahead,
    },
    PositionReported {
        position: Duration,
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
                    Err(Unhandled)
                }
            },
            PlayerMessage::Start { track, stamp } => {
                Ok(self.start(track, StartOrigin::User(stamp)))
            }
            PlayerMessage::Loaded { duration, anchor } => self.loaded(duration, anchor),
            PlayerMessage::Error(error) => self.failed(&error),
            PlayerMessage::SpeedChanged(anchor) => match self {
                Player::Playing { playhead, .. } => {
                    *playhead = Playhead::anchored(
                        playhead.position_at(anchor.started_at),
                        anchor.started_at,
                        anchor.speed,
                    );
                    Ok(Cmd::none())
                }
                Player::Loading(..) | Player::Paused { .. } | Player::Stopped => {
                    Err(Unhandled)
                }
            },
            PlayerMessage::LookaheadReached {
                position,
                lookahead,
            } => self.positioned(position, lookahead),
            PlayerMessage::PositionReported { position, now } => {
                self.reported(position, now)
            }
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
        let track_load = TrackLoad::for_track(&track, origin.stamp().revision);
        let load_effect = Effect::Audio(AudioCmd::Load(track_load));
        let cmd = origin
            .stop()
            .into_iter()
            .chain([load_effect])
            .chain(handover_effects(
                &track,
                PlaybackChange::Play,
                origin.stamp().anchor.started_at,
            ))
            .collect();
        *self = Player::Loading(track);
        cmd
    }
}

pub(crate) fn update_player(
    playback_parts: &mut PlaybackParts<'_>,
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let seeks = matches!(message, PlayerMessage::Seek { .. });
    let candidate = playback_parts.revisions.effects.next();
    let cmd = playback_parts.player.transition(message)?;
    playback_parts.revisions.effects = candidate;
    let armed = if seeks {
        timer(playback_parts, now)
    } else {
        arm(playback_parts, now)
    };
    Ok(cmd.then(armed))
}

pub(crate) fn duration_of(player: &Player) -> Duration {
    player
        .current()
        .and_then(|track| track.duration())
        .unwrap_or(Duration::ZERO)
}

pub(crate) fn lookahead(playback_parts: &PlaybackParts<'_>, now: Moment) -> Lookahead {
    let ab_loop = match playback_parts.transport.ab_loop {
        Some(AbLoop::BothMarked {
            loop_start,
            loop_end,
        }) => Some((loop_start, loop_end)),
        Some(AbLoop::StartMarked(_)) | None => None,
    };
    Lookahead {
        ab_loop,
        next: successor(playback_parts.playlist, playback_parts.queue)
            .track()
            .cloned(),
        duration: duration_of(playback_parts.player),
        now,
        revision: playback_parts.revisions.effects.next(),
        cover_side: crate::update::cover_side(
            playback_parts.workspace,
            playback_parts.settings,
        ),
    }
}

pub(crate) fn arm(playback_parts: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { playhead, .. } = &*playback_parts.player else {
        return Cmd::none();
    };
    Cmd::from(Effect::Macos(MacosCmd::SetPosition(
        playhead.position_at(now),
    )))
    .then(timer(playback_parts, now))
}

fn timer(playback_parts: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { playhead, .. } = &*playback_parts.player else {
        return Cmd::none();
    };
    next_decision(*playhead, &lookahead(playback_parts, now)).map_or(
        Cmd::none(),
        |delay| {
            Effect::After {
                delay,
                timer: Timer::Lookahead(playback_parts.revisions.issue_lookahead()),
            }
            .into()
        },
    )
}

pub(crate) fn stopped_effects() -> Cmd {
    PlaybackChange::Stop
        .effects()
        .into_iter()
        .chain([Effect::Macos(MacosCmd::NowPlaying(None))])
        .collect()
}
