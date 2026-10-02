use std::{mem, sync::Arc, time::Duration};

use crate::{
    cmd::{Cmd, PlaybackChange},
    domain::{Moment, PausedBy, Player, Playhead, Preload, Track},
    update::player::{Anchor, PlayerError, Stamp, StartOrigin, seek_effect},
};

impl Player {
    pub(crate) fn toggle(
        &mut self,
        current: Option<Arc<Track>>,
        stamp: Stamp,
    ) -> Result<Cmd, PlayerError> {
        match self {
            Player::Stopped => {
                let track = current.ok_or(PlayerError::Stopped)?;
                Ok(self.start(track, StartOrigin::User(stamp)))
            }
            Player::Loading { .. } => Err(PlayerError::Loading),
            Player::Playing { .. } => {
                Ok(self.pause(stamp.anchor.since, PausedBy::Listener))
            }
            Player::Paused { .. } => Ok(self.resume(stamp.anchor)),
        }
    }

    pub(crate) fn seek(
        &mut self,
        target: Duration,
        now: Moment,
    ) -> Result<Cmd, PlayerError> {
        match self {
            Player::Playing { head, preload, .. } => {
                *head = Playhead::anchored(target, now, head.speed);
                *preload = mem::replace(preload, Preload::None).seek_reset();
                Ok(seek_effect(target))
            }
            Player::Paused { at, .. } => {
                *at = target;
                Ok(seek_effect(target))
            }
            Player::Loading { .. } | Player::Stopped => Err(self.refusal()),
        }
    }

    pub(crate) fn pause(&mut self, now: Moment, by: PausedBy) -> Cmd {
        match mem::replace(self, Player::Stopped) {
            Player::Playing { track, head, .. } => {
                *self = Player::Paused {
                    track,
                    at: head.position_at(now),
                    by,
                };
                PlaybackChange::Pause.cued()
            }
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => {
                *self = other;
                Cmd::None
            }
        }
    }

    pub(crate) fn release(&mut self, anchor: Anchor) -> Cmd {
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
            | Player::Loading { .. }
            | Player::Stopped => Cmd::None,
        }
    }

    fn resume(&mut self, anchor: Anchor) -> Cmd {
        match mem::replace(self, Player::Stopped) {
            Player::Paused { track, at, .. } => {
                *self = Player::Playing {
                    track,
                    head: Playhead::anchored(at, anchor.since, anchor.speed),
                    preload: Preload::None,
                };
                PlaybackChange::Play.cued()
            }
            other @ (Player::Playing { .. }
            | Player::Loading { .. }
            | Player::Stopped) => {
                *self = other;
                Cmd::None
            }
        }
    }
}
