use std::{fmt, time::Duration};

use crate::domain::Bounded;

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
    pub fn new(length: Duration) -> Option<Self> {
        (length <= Self::MAX).then_some(Self(length))
    }

    #[must_use]
    pub const fn value(self) -> Duration {
        self.0
    }

    pub fn step_up(self) -> Self {
        Self::clamped(self.0.saturating_add(Self::STEP))
    }

    pub fn step_down(self) -> Self {
        Self::clamped(self.0.saturating_sub(Self::STEP))
    }

    pub fn step(self, delta: i64) -> Self {
        if delta < 0 {
            self.step_down()
        } else {
            self.step_up()
        }
    }
}

impl fmt::Display for Crossfade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "crossfade {length:?} is out of range (must be <= {:?})",
    Crossfade::MAX
)]
pub struct CrossfadeOutOfRange {
    pub length: Duration,
}

impl TryFrom<Duration> for Crossfade {
    type Error = CrossfadeOutOfRange;

    fn try_from(length: Duration) -> Result<Self, Self::Error> {
        Self::new(length).ok_or(CrossfadeOutOfRange { length })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::{Bounded, crossfade::Crossfade};

    #[rstest]
    #[case::at_the_ceiling(Crossfade::MAX, Some(Crossfade::MAX))]
    #[case::past_the_ceiling(Duration::from_secs(11), None)]
    fn new_rejects_anything_above_max(
        #[case] length: Duration,
        #[case] expected: Option<Duration>,
    ) {
        assert_eq!(Crossfade::new(length).map(Crossfade::value), expected);
    }

    #[rstest]
    #[case::saturates_above_the_ceiling(Duration::from_secs(20), Crossfade::MAX)]
    #[case::saturates_below_the_floor(Duration::ZERO, Duration::ZERO)]
    fn clamped_saturates_both_directions(
        #[case] raw: Duration,
        #[case] expected: Duration,
    ) {
        assert_eq!(Crossfade::clamped(raw).value(), expected);
    }

    #[rstest]
    #[case::step_up_saturates_at_the_ceiling(
        Crossfade::clamped(Crossfade::MAX),
        Crossfade::step_up,
        Crossfade::MAX
    )]
    #[case::step_down_saturates_at_the_floor(
        Crossfade::default(),
        Crossfade::step_down,
        Duration::ZERO
    )]
    #[case::step_up_from_default(
        Crossfade::default(),
        Crossfade::step_up,
        Duration::from_millis(500)
    )]
    fn step_saturates_at_bounds(
        #[case] start: Crossfade,
        #[case] step: fn(Crossfade) -> Crossfade,
        #[case] expected: Duration,
    ) {
        assert_eq!(step(start).value(), expected);
    }

    #[rstest]
    #[case::positive_delta_steps_up(
        Crossfade::default(),
        1,
        Duration::from_millis(500)
    )]
    #[case::negative_delta_clamps_at_the_floor(Crossfade::default(), -1, Duration::ZERO)]
    #[case::positive_delta_clamps_at_the_ceiling(
        Crossfade::clamped(Crossfade::MAX),
        1,
        Crossfade::MAX
    )]
    fn step_reads_only_the_sign_of_delta(
        #[case] start: Crossfade,
        #[case] delta: i64,
        #[case] expected: Duration,
    ) {
        assert_eq!(start.step(delta).value(), expected);
    }

    #[test]
    fn default_is_gapless() {
        assert_eq!(Crossfade::default().value(), Duration::ZERO);
    }

    #[rstest]
    #[case::within_range(Duration::from_secs(3), Some(Duration::from_secs(3)))]
    #[case::out_of_range(Duration::from_secs(11), None)]
    fn try_from_duration_round_trips_through_the_valid_range(
        #[case] raw: Duration,
        #[case] expected: Option<Duration>,
    ) {
        assert_eq!(
            Crossfade::try_from(raw).map(Crossfade::value).ok(),
            expected
        );
    }
}
