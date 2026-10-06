use std::{sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::{playhead::Playhead, time::Moment, track::Track};

#[derive(Debug, Clone, Default, PartialEq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Player {
    #[default]
    Stopped,
    Loading(Arc<Track>),
    Playing {
        track: Arc<Track>,
        playhead: Playhead,
        preloaded: Option<Arc<Track>>,
    },
    Paused {
        track: Arc<Track>,
        position: Duration,
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
            Self::Loading(track)
            | Self::Playing { track, .. }
            | Self::Paused { track, .. } => Some(track),
        }
    }

    #[must_use]
    pub fn position_at(&self, now: Moment) -> Duration {
        match self {
            Self::Stopped | Self::Loading(..) => Duration::ZERO,
            Self::Paused { position, .. } => *position,
            Self::Playing { playhead, .. } => playhead.position_at(now),
        }
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Playing { .. })
    }

    #[must_use]
    pub(crate) fn preloaded(&self) -> Option<&Arc<Track>> {
        match self {
            Self::Playing { preloaded, .. } => preloaded.as_ref(),
            Self::Stopped | Self::Loading(..) | Self::Paused { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbLoop {
    StartMarked(Duration),
    BothMarked {
        loop_start: Duration,
        loop_end: Duration,
    },
}

impl AbLoop {
    #[must_use]
    pub fn mark(current: Option<Self>, position: Duration) -> Option<Self> {
        match current {
            None => Some(Self::StartMarked(position)),
            Some(Self::StartMarked(a)) if position > a => Some(Self::BothMarked {
                loop_start: a,
                loop_end: position,
            }),
            Some(loop_ @ Self::StartMarked(_)) => Some(loop_),
            Some(Self::BothMarked { .. }) => None,
        }
    }
}

#[cfg(test)]
mod ab_loop_tests {
    use std::time::Duration;

    use crate::domain::player::AbLoop;

    #[test]
    fn ab_loop_mark_sets_b_only_after_a() {
        let loop_start = Duration::from_secs(10);
        let marked_loop = AbLoop::mark(None, loop_start);

        assert_eq!(AbLoop::mark(marked_loop, loop_start), marked_loop);
        assert_eq!(
            AbLoop::mark(marked_loop, Duration::from_secs(9)),
            marked_loop
        );
        assert_eq!(
            AbLoop::mark(marked_loop, Duration::from_secs(11)),
            Some(AbLoop::BothMarked {
                loop_start,
                loop_end: Duration::from_secs(11),
            })
        );
    }
}
