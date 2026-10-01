use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{Cmd, PlaybackChange},
    domain::{Moment, Pause, Player, Playhead, Preload, Track},
    update::player::{Anchor, Stamp, StartOrigin, Transition, seek_effect, start},
};

impl Player {
    pub(crate) fn toggle(
        self,
        current: Option<Arc<Track>>,
        stamp: Stamp,
    ) -> Transition {
        let anchor = stamp.anchor;
        match self {
            Player::Stopped => current.map_or_else(
                || Player::Stopped.refuse(),
                |track| Ok(start(track, StartOrigin::User, stamp)),
            ),
            loading @ Player::Loading { .. } => loading.refuse(),
            playing @ Player::Playing { .. } => {
                playing.pause(anchor.since, Pause::ByListener)
            }
            Player::Paused { track, at, .. } => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(at, anchor.since, anchor.speed),
                    preload: Preload::None,
                },
                PlaybackChange::Play.cued(),
            )),
        }
    }

    pub(crate) fn seek(self, target: Duration, now: Moment) -> Transition {
        match self {
            Player::Playing {
                track,
                head,
                preload,
            } => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(target, now, head.speed),
                    preload: preload.seek_reset(),
                },
                seek_effect(target),
            )),
            Player::Paused { track, pause, .. } => Ok((
                Player::Paused {
                    track,
                    at: target,
                    pause,
                },
                seek_effect(target),
            )),
            other @ (Player::Loading { .. } | Player::Stopped) => other.refuse(),
        }
    }

    pub(crate) fn pause(self, now: Moment, pause: Pause) -> Transition {
        match self {
            Player::Playing { track, head, .. } => Ok((
                Player::Paused {
                    track,
                    at: head.position_at(now),
                    pause,
                },
                PlaybackChange::Pause.cued(),
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => Ok((other, Cmd::None)),
        }
    }

    pub(crate) fn release(self, anchor: Anchor) -> Transition {
        match self {
            Player::Paused {
                track,
                at,
                pause: Pause::ByOverlay,
            } => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(at, anchor.since, anchor.speed),
                    preload: Preload::None,
                },
                PlaybackChange::Play.cued(),
            )),
            other @ (Player::Paused {
                pause: Pause::ByListener,
                ..
            }
            | Player::Playing { .. }
            | Player::Loading { .. }
            | Player::Stopped) => Ok((other, Cmd::None)),
        }
    }
}
