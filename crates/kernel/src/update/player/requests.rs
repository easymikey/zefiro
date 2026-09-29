use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{Cmd, PlaybackChange},
    domain::{Moment, Pause, Player, Playhead, Preload, Track},
    update::player::{Anchor, Resume, StartOrigin, Transition, seek_effect, start},
};

impl Player {
    pub(crate) fn toggle(
        self,
        current: Option<Arc<Track>>,
        resume: Resume,
    ) -> Transition {
        match self {
            Player::Stopped => current.map_or_else(
                || Player::Stopped.refuse(),
                |track| Ok(start(track, StartOrigin::User)),
            ),
            loading @ Player::Loading { .. } => loading.refuse(),
            Player::Playing { track, head, .. } => Ok((
                Player::Paused {
                    track,
                    at: head.position_at(resume.anchor.now),
                    pause: Pause::ByListener,
                },
                PlaybackChange::Pause.cued(),
            )),
            Player::Paused { track, at, .. } => Ok((
                Player::Playing {
                    track,
                    head: Playhead::anchored(
                        at,
                        resume.anchor.now,
                        resume.anchor.speed,
                    ),
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

    pub(crate) fn sleep_fired(self, now: Moment) -> Transition {
        match self {
            Player::Playing { track, head, .. } => Ok((
                Player::Paused {
                    track,
                    at: head.position_at(now),
                    pause: Pause::ByListener,
                },
                PlaybackChange::Pause.cued(),
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => Ok((other, Cmd::None)),
        }
    }

    pub(crate) fn hold(self, now: Moment) -> Transition {
        match self {
            Player::Playing { track, head, .. } => Ok((
                Player::Paused {
                    track,
                    at: head.position_at(now),
                    pause: Pause::ByOverlay,
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
                    head: Playhead::anchored(at, anchor.now, anchor.speed),
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
