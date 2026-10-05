use std::{fmt, time::Duration};

use crate::domain::{bounded::Bounded, direction::Direction};

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Crossfade(Duration);

impl Bounded for Crossfade {
    type Raw = Duration;

    const MIN: Duration = Duration::ZERO;
    const MAX: Duration = Duration::from_secs(10);

    fn within_bounds(length: Duration) -> Self {
        Self(length)
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
    #[error("crossfade {value:?} is above {max:?}")]
    OutOfRange { value: Duration, max: Duration },
}

impl TryFrom<Duration> for Crossfade {
    type Error = CrossfadeError;

    fn try_from(length: Duration) -> Result<Self, Self::Error> {
        (length <= Self::MAX).then_some(Self(length)).ok_or(
            CrossfadeError::OutOfRange {
                value: length,
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
    #[case::saturates_above_the_ceiling(Duration::from_secs(20), Crossfade::MAX)]
    #[case::saturates_below_the_floor(Duration::ZERO, Duration::ZERO)]
    fn clamped_saturates_both_directions(
        #[case] raw: Duration,
        #[case] expected: Duration,
    ) {
        assert_eq!(Crossfade::clamped(raw).get(), expected);
    }

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
        #[case] start: Crossfade,
        #[case] direction: Direction,
        #[case] expected: Duration,
    ) {
        assert_eq!(start.step(direction).get(), expected);
    }

    #[test]
    fn default_is_gapless() {
        assert_eq!(Crossfade::default().get(), Duration::ZERO);
    }

    #[rstest]
    #[case::at_the_ceiling(Crossfade::MAX, Some(Crossfade::MAX))]
    #[case::within_range(Duration::from_secs(3), Some(Duration::from_secs(3)))]
    #[case::out_of_range(Duration::from_secs(11), None)]
    fn try_from_duration_round_trips_through_the_valid_range(
        #[case] raw: Duration,
        #[case] expected: Option<Duration>,
    ) {
        assert_eq!(Crossfade::try_from(raw).map(Crossfade::get).ok(), expected);
    }
}
