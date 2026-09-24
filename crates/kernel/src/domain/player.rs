use std::{sync::Arc, time::Duration};

use strum::IntoStaticStr;

use crate::domain::Track;

const LISTENING_STEP_LIMIT: Duration = Duration::from_secs(1);

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
        at: Duration,
        preload: Preload,
    },
    Paused {
        track: Arc<Track>,
        at: Duration,
        pause: Pause,
    },
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMotion {
    Live,
    Held,
    Decaying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    ByListener,
    ByOverlay,
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
    pub fn position(&self) -> Duration {
        match self {
            Self::Stopped => Duration::ZERO,
            Self::Loading { at, .. }
            | Self::Playing { at, .. }
            | Self::Paused { at, .. } => *at,
        }
    }

    #[must_use]
    pub(crate) fn listened(&self, reported: Duration) -> Duration {
        match self {
            Self::Playing { at, .. } => Some(reported.saturating_sub(*at))
                .filter(|step| *step <= LISTENING_STEP_LIMIT)
                .unwrap_or_default(),
            Self::Stopped | Self::Loading { .. } | Self::Paused { .. } => {
                Duration::ZERO
            }
        }
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Playing { .. })
    }

    pub fn playback_motion(&self) -> PlaybackMotion {
        match self {
            Self::Playing { .. } => PlaybackMotion::Live,
            Self::Paused { .. } => PlaybackMotion::Held,
            Self::Stopped | Self::Loading { .. } => PlaybackMotion::Decaying,
        }
    }

    #[must_use]
    pub fn preloaded(&self) -> Option<&Arc<Track>> {
        match self {
            Self::Playing { preload, .. } => preload.track(),
            Self::Stopped | Self::Loading { .. } | Self::Paused { .. } => None,
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
    pub fn track(&self) -> Option<&Arc<Track>> {
        match self {
            Self::None => None,
            Self::Queued(track) | Self::Stale(track) => Some(track),
        }
    }

    pub fn seek_reset(self) -> Self {
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

#[cfg(test)]
mod listened_tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::domain::{
        AudioFormat,
        Pause,
        Tags,
        Track,
        player::{Player, Preload},
    };

    fn track() -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path("a.mp3")
                .duration(Duration::from_secs(300))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

    fn playing_at(millis: u64) -> Player {
        Player::Playing {
            track: track(),
            at: Duration::from_millis(millis),
            preload: Preload::None,
        }
    }

    #[rstest]
    #[case::a_regular_step_counts(playing_at(1_000), 1_100, 100)]
    #[case::a_step_at_the_limit_counts(playing_at(1_000), 2_000, 1_000)]
    #[case::a_jump_past_the_limit_is_a_seek(playing_at(1_000), 60_000, 0)]
    #[case::a_backward_report_counts_nothing(playing_at(5_000), 4_900, 0)]
    #[case::a_stopped_player_counts_nothing(Player::Stopped, 100, 0)]
    #[case::a_paused_player_counts_nothing(
        Player::Paused { track: track(), at: Duration::ZERO, pause: Pause::ByListener },
        100,
        0
    )]
    fn listened_counts_only_small_forward_steps_while_playing(
        #[case] player: Player,
        #[case] reported_millis: u64,
        #[case] listened_millis: u64,
    ) {
        assert_eq!(
            player.listened(Duration::from_millis(reported_millis)),
            Duration::from_millis(listened_millis)
        );
    }
}
