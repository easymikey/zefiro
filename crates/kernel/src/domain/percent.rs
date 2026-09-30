use std::fmt;

use crate::domain::Bounded;

const MAX: u8 = 100;

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Percent(u8);

impl Bounded for Percent {
    type Raw = u8;

    const MIN: u8 = 0;
    const MAX: u8 = MAX;

    fn within_bounds(percent: u8) -> Self {
        Self(percent)
    }
}

impl Percent {
    #[must_use]
    pub const fn new(percent: u8) -> Option<Self> {
        if percent <= MAX {
            Some(Self(percent))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn ratio(self) -> f32 {
        f32::from(self.0) / f32::from(MAX)
    }

    pub fn step(self, delta: i8) -> Self {
        Self::clamped(self.0.saturating_add_signed(delta))
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Percent> for u8 {
    fn from(percent: Percent) -> Self {
        percent.0
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{Bounded, percent::Percent};

    #[rstest]
    #[case::at_the_ceiling_is_accepted(100, Some(100))]
    #[case::past_the_ceiling_is_rejected(101, None)]
    fn new_rejects_anything_above_100(#[case] raw: u8, #[case] expected: Option<u8>) {
        assert_eq!(Percent::new(raw).map(Percent::value), expected);
    }

    #[rstest]
    #[case::floor(0, 0.0)]
    #[case::middle(50, 0.5)]
    #[case::ceiling(100, 1.0)]
    fn ratio_scales_to_the_unit_range(#[case] raw: u8, #[case] expected: f32) {
        assert_eq!(Percent::clamped(raw).ratio(), expected);
    }

    #[rstest]
    #[case::saturates_at_the_ceiling(98, 5, 100)]
    #[case::saturates_at_the_floor(3, -5, 0)]
    #[case::stays_within_bounds_upward(50, 10, 60)]
    #[case::stays_at_the_floor_when_already_there(0, -1, 0)]
    fn clamp_saturates_a_delta_both_directions(
        #[case] start: u8,
        #[case] delta: i8,
        #[case] expected: u8,
    ) {
        assert_eq!(Percent::clamped(start).step(delta).value(), expected);
    }

    #[test]
    fn display_prints_the_bare_number() {
        assert_eq!(Percent::clamped(42).to_string(), "42");
    }
}
