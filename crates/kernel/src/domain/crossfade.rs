use std::{fmt, time::Duration};

use crate::domain::{bounded::Bounded, direction::Direction};

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Crossfade(Duration);

impl Bounded for Crossfade {
    type Raw = Duration;

    const MIN: Duration = Duration::ZERO;
    const MAX: Duration = Duration::from_secs(10);

    fn within_bounds(duration: Duration) -> Self {
        Self(duration)
    }
}

impl Crossfade {
    pub const STEP: Duration = Duration::from_millis(500);

    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }

    pub(crate) fn step(self, direction: Direction) -> Self {
        match direction {
            Direction::Next => Self::clamped(self.0.saturating_add(Self::STEP)),
            Direction::Previous => Self::clamped(self.0.saturating_sub(Self::STEP)),
        }
    }
}

impl fmt::Display for Crossfade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CrossfadeError {
    #[error("crossfade {duration:?} is above {max:?}")]
    OutOfRange { duration: Duration, max: Duration },
}

impl TryFrom<Duration> for Crossfade {
    type Error = CrossfadeError;

    fn try_from(length: Duration) -> Result<Self, Self::Error> {
        (length <= Self::MAX).then_some(Self(length)).ok_or(
            CrossfadeError::OutOfRange {
                duration: length,
                max: Self::MAX,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::{bounded::Bounded, crossfade::Crossfade, direction::Direction};

    #[rstest]
    #[case::next_steps_up(
        Crossfade::default(),
        Direction::Next,
        Duration::from_millis(500)
    )]
    #[case::previous_clamps_at_the_floor(
        Crossfade::default(),
        Direction::Previous,
        Duration::ZERO
    )]
    #[case::next_clamps_at_the_ceiling(
        Crossfade::clamped(Crossfade::MAX),
        Direction::Next,
        Crossfade::MAX
    )]
    fn step_follows_the_direction(
        #[case] crossfade: Crossfade,
        #[case] direction: Direction,
        #[case] expected: Duration,
    ) {
        assert_eq!(crossfade.step(direction).get(), expected);
    }
}
