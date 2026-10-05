use std::fmt;

use crate::domain::{bounded::Bounded, direction::Direction};

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speed(f32);

impl Bounded for Speed {
    type Raw = f32;

    const MIN: f32 = 0.25;
    const MAX: f32 = 4.0;

    fn within_bounds(rate: f32) -> Self {
        Self(rate)
    }

    fn clamped(rate: f32) -> Self {
        if rate.is_finite() {
            return Self(rate.clamp(Self::MIN, Self::MAX));
        }
        Self(1.0)
    }
}

impl Speed {
    pub const STEP: f32 = 0.25;

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    pub(crate) fn step(self, direction: Direction) -> Self {
        match direction {
            Direction::Next => Self::clamped(self.0 + Self::STEP),
            Direction::Previous => Self::clamped(self.0 - Self::STEP),
        }
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self(1.0)
    }
}

impl fmt::Display for Speed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted = format!("{:.2}", self.0);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        write!(f, "{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{bounded::Bounded, direction::Direction, speed::Speed};

    #[rstest]
    #[case::saturates_above_the_ceiling(10.0, 4.0)]
    #[case::saturates_below_the_floor(0.0, 0.25)]
    #[case::nan_resets_to_unity(f32::NAN, 1.0)]
    fn clamped_saturates(#[case] raw: f32, #[case] expected: f32) {
        assert_eq!(Speed::clamped(raw).get(), expected);
    }

    #[test]
    fn default_is_unity() {
        assert_eq!(Speed::default().get(), 1.0);
    }

    #[rstest]
    #[case::next_saturates_at_the_ceiling(Speed::clamped(4.0), Direction::Next, 4.0)]
    #[case::previous_saturates_at_the_floor(
        Speed::clamped(0.25),
        Direction::Previous,
        0.25
    )]
    #[case::next_from_default(Speed::default(), Direction::Next, 1.25)]
    #[case::previous_from_default(Speed::default(), Direction::Previous, 0.75)]
    fn step_saturates_at_bounds(
        #[case] start: Speed,
        #[case] direction: Direction,
        #[case] expected: f32,
    ) {
        assert_eq!(start.step(direction).get(), expected);
    }

    #[test]
    fn display_reports_the_default_as_one() {
        assert_eq!(Speed::default().to_string(), "1");
    }

    #[rstest]
    #[case::away_from_default(1.25, "1.25")]
    #[case::at_default(1.0, "1")]
    #[case::whole_number_trims_to_its_integer(2.0, "2")]
    #[case::one_trailing_zero_is_trimmed(0.5, "0.5")]
    fn display_trims_trailing_zeros(#[case] raw: f32, #[case] expected: &str) {
        assert_eq!(Speed::clamped(raw).to_string(), expected);
    }
}
