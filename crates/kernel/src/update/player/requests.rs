use std::{mem, sync::Arc, time::Duration};

use crate::{
    cmd::Cmd,
    domain::{
        cue::PlaybackChange,
        player::{PausedBy, Player},
        playhead::Playhead,
        time::Moment,
        track::Track,
    },
    update::{
        machine::Unhandled,
        player::stamp::{Anchor, Stamp, StartOrigin},
    },
};

impl Player {
    pub(crate) fn toggle(
        &mut self,
        current: Option<Arc<Track>>,
        stamp: Stamp,
    ) -> Result<Cmd, Unhandled> {
        match self {
            Player::Stopped => {
                let track = current.ok_or(Unhandled)?;
                Ok(self.start(track, StartOrigin::User(stamp)))
            }
            Player::Loading(..) => Err(Unhandled),
            Player::Playing { .. } => {
                self.pause(stamp.anchor.started_at, PausedBy::Listener)
            }
            Player::Paused { .. } => self.resume(stamp.anchor),
        }
    }

    pub(crate) fn seek(
        &mut self,
        target: Duration,
        now: Moment,
    ) -> Result<(), Unhandled> {
        match self {
            Player::Playing { playhead, .. } => {
                *playhead = Playhead::anchored(target, now, playhead.speed);
                Ok(())
            }
            Player::Paused { position, .. } => {
                *position = target;
                Ok(())
            }
            Player::Loading(..) | Player::Stopped => Err(Unhandled),
        }
    }

    pub(crate) fn pause(
        &mut self,
        now: Moment,
        by: PausedBy,
    ) -> Result<Cmd, Unhandled> {
        match mem::replace(self, Player::Stopped) {
            Player::Playing {
                track, playhead, ..
            } => {
                *self = Player::Paused {
                    track,
                    position: playhead.position_at(now),
                    by,
                };
                Ok(PlaybackChange::Pause.cued())
            }
            other @ (Player::Paused { .. } | Player::Loading(..) | Player::Stopped) => {
                *self = other;
                Err(Unhandled)
            }
        }
    }

    pub(crate) fn release(&mut self, anchor: Anchor) -> Result<Cmd, Unhandled> {
        match self {
            Player::Paused {
                by: PausedBy::Overlay,
                ..
            } => self.resume(anchor),
            Player::Paused {
                by: PausedBy::Listener,
                ..
            }
            | Player::Playing { .. }
            | Player::Loading(..)
            | Player::Stopped => Err(Unhandled),
        }
    }

    fn resume(&mut self, anchor: Anchor) -> Result<Cmd, Unhandled> {
        match mem::replace(self, Player::Stopped) {
            Player::Paused {
                track, position, ..
            } => {
                *self = Player::Playing {
                    track,
                    playhead: Playhead::anchored(
                        position,
                        anchor.started_at,
                        anchor.speed,
                    ),
                    preloaded: None,
                };
                Ok(PlaybackChange::Play.cued())
            }
            other
            @ (Player::Playing { .. } | Player::Loading(..) | Player::Stopped) => {
                *self = other;
                Err(Unhandled)
            }
        }
    }
}
