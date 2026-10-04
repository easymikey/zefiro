use std::{sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{playhead::Playhead, speed::Speed, time::Moment, track::Track};

#[derive(Debug, Clone, Default, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Player {
    #[default]
    Stopped,
    Loading {
        track: Arc<Track>,
        at: Duration,
    },
    Playing {
        track: Arc<Track>,
        head: Playhead,
        preload: Preload,
    },
    Paused {
        track: Arc<Track>,
        at: Duration,
        by: PausedBy,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PausedBy {
    Listener,
    Overlay,
}

impl Player {
    #[must_use]
    pub fn current(&self) -> Option<&Arc<Track>> {
        match self {
            Self::Stopped => None,
            Self::Loading { track, .. }
            | Self::Playing { track, .. }
            | Self::Paused { track, .. } => Some(track),
        }
    }

    #[must_use]
    pub fn position_at(&self, now: Moment) -> Duration {
        match self {
            Self::Stopped => Duration::ZERO,
            Self::Loading { at, .. } | Self::Paused { at, .. } => *at,
            Self::Playing { head, .. } => head.position_at(now),
        }
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Playing { .. })
    }

    #[must_use]
    pub(crate) fn preloaded(&self) -> Option<&Arc<Track>> {
        match self {
            Self::Playing { preload, .. } => preload.track(),
            Self::Stopped | Self::Loading { .. } | Self::Paused { .. } => None,
        }
    }

    #[must_use]
    pub(crate) fn reanchored(self, now: Moment, speed: Speed) -> Self {
        match self {
            Self::Playing {
                track,
                head,
                preload,
            } => Self::Playing {
                track,
                head: Playhead::anchored(head.position_at(now), now, speed),
                preload,
            },
            other @ (Self::Stopped | Self::Loading { .. } | Self::Paused { .. }) => {
                other
            }
        }
    }
}

#[must_use]
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Preload {
    #[default]
    None,
    Queued(Arc<Track>),
    Stale(Arc<Track>),
}

impl Preload {
    #[must_use]
    pub(crate) fn track(&self) -> Option<&Arc<Track>> {
        match self {
            Self::None => None,
            Self::Queued(track) | Self::Stale(track) => Some(track),
        }
    }

    pub(crate) fn seek_reset(self) -> Self {
        match self {
            Self::Queued(track) => Self::Stale(track),
            other @ (Self::None | Self::Stale(_)) => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbLoop {
    AOnly(Duration),
    Full { a: Duration, b: Duration },
}

impl AbLoop {
    #[must_use]
    pub fn mark(current: Option<Self>, position: Duration) -> Option<Self> {
        match current {
            None => Some(Self::AOnly(position)),
            Some(Self::AOnly(a)) if position > a => Some(Self::Full { a, b: position }),
            Some(loop_ @ Self::AOnly(_)) => Some(loop_),
            Some(Self::Full { .. }) => None,
        }
    }
}

#[cfg(test)]
mod ab_loop_tests {
    use std::time::Duration;

    use crate::domain::player::AbLoop;

    #[test]
    fn ab_loop_mark_sets_b_only_after_a() {
        let a = Duration::from_secs(10);
        let marked_a = AbLoop::mark(None, a);

        assert_eq!(AbLoop::mark(marked_a, a), marked_a);
        assert_eq!(AbLoop::mark(marked_a, Duration::from_secs(9)), marked_a);
        assert_eq!(
            AbLoop::mark(marked_a, Duration::from_secs(11)),
            Some(AbLoop::Full {
                a,
                b: Duration::from_secs(11),
            })
        );
    }
}
