use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{Cmd, PlaybackChange},
    domain::{Pause, Percent, Player, Preload, Track},
    update::player::{StartOrigin, Transition, seek_effect, start},
};

impl Player {
    pub(super) fn toggle(
        self,
        current: Option<Arc<Track>>,
        volume: Percent,
    ) -> Transition {
        match self {
            Player::Stopped => current.map_or_else(
                || Player::Stopped.refuse(),
                |track| Ok(start(track, volume, StartOrigin::User)),
            ),
            loading @ Player::Loading { .. } => loading.refuse(),
            Player::Playing { track, at, .. } => Ok((
                Player::Paused {
                    track,
                    at,
                    pause: Pause::ByListener,
                },
                PlaybackChange::Pause.cued(),
            )),
            Player::Paused { track, at, .. } => Ok((
                Player::Playing {
                    track,
                    at,
                    preload: Preload::None,
                },
                PlaybackChange::Play.cued(),
            )),
        }
    }

    pub(super) fn seek(self, target: Duration) -> Transition {
        match self {
            Player::Playing { track, preload, .. } => Ok((
                Player::Playing {
                    track,
                    at: target,
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

    pub(super) fn sleep_fired(self) -> Transition {
        match self {
            Player::Playing { track, at, .. } => Ok((
                Player::Paused {
                    track,
                    at,
                    pause: Pause::ByListener,
                },
                PlaybackChange::Pause.cued(),
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => Ok((other, Cmd::None)),
        }
    }

    pub(super) fn hold(self) -> Transition {
        match self {
            Player::Playing { track, at, .. } => Ok((
                Player::Paused {
                    track,
                    at,
                    pause: Pause::ByOverlay,
                },
                PlaybackChange::Pause.cued(),
            )),
            other @ (Player::Paused { .. }
            | Player::Loading { .. }
            | Player::Stopped) => Ok((other, Cmd::None)),
        }
    }

    pub(super) fn release(self) -> Transition {
        match self {
            Player::Paused {
                track,
                at,
                pause: Pause::ByOverlay,
            } => Ok((
                Player::Playing {
                    track,
                    at,
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
