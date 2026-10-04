use std::fmt;

use crate::domain::{Bounded, Direction};

const MAX: u8 = 100;
const VOLUME_STEP: i8 = 5;

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
    pub const fn get(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn ratio(self) -> f32 {
        f32::from(self.0) / f32::from(MAX)
    }

    pub fn from_ratio(ratio: f32) -> Self {
        let ratio = if ratio.is_nan() { 0.0 } else { ratio };
        let scaled = ratio.clamp(0.0, 1.0) * f32::from(MAX);
        Self(
            (0..=MAX)
                .find(|step| f32::from(*step) + 0.5 > scaled)
                .unwrap_or(MAX),
        )
    }

    pub fn step(self, delta: i8) -> Self {
        Self::clamped(self.0.saturating_add_signed(delta))
    }

    pub fn step_by(self, direction: Direction) -> Self {
        match direction {
            Direction::Next => self.step(VOLUME_STEP),
            Direction::Previous => self.step(-VOLUME_STEP),
        }
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
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
        assert_eq!(Percent::new(raw).map(Percent::get), expected);
    }

    #[rstest]
    #[case::floor(0, 0.0)]
    #[case::middle(50, 0.5)]
    #[case::ceiling(100, 1.0)]
    fn ratio_scales_to_the_unit_range(#[case] raw: u8, #[case] expected: f32) {
        assert_eq!(Percent::clamped(raw).ratio(), expected);
    }

    #[rstest]
    #[case::floor(0.0, 0)]
    #[case::rounds_down(0.404, 40)]
    #[case::rounds_up(0.406, 41)]
    #[case::ceiling(1.0, 100)]
    #[case::clamps_above_one(1.7, 100)]
    #[case::clamps_below_zero(-0.2, 0)]
    #[case::not_a_number_is_zero(f32::NAN, 0)]
    fn from_ratio_rounds_and_clamps(#[case] ratio: f32, #[case] percent: u8) {
        assert_eq!(Percent::from_ratio(ratio), Percent::clamped(percent));
    }

    #[rstest]
    #[case::silence(0)]
    #[case::a_sliver(1)]
    #[case::two_fifths(40)]
    #[case::almost_full(99)]
    #[case::full(100)]
    fn ratio_round_trips_every_percent(#[case] percent: u8) {
        let volume = Percent::clamped(percent);
        assert_eq!(Percent::from_ratio(volume.ratio()), volume);
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
        assert_eq!(Percent::clamped(start).step(delta).get(), expected);
    }

    #[test]
    fn display_prints_the_bare_number() {
        assert_eq!(Percent::clamped(42).to_string(), "42");
    }
}
