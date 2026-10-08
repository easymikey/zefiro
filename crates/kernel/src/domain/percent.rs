use std::fmt;

use crate::domain::{bounded::Bounded, direction::Direction};

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

    pub(crate) fn step(self, direction: Direction) -> Self {
        let delta = match direction {
            Direction::Next => VOLUME_STEP,
            Direction::Previous => -VOLUME_STEP,
        };
        Self::clamped(self.0.saturating_add_signed(delta))
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{bounded::Bounded, percent::Percent};

    #[test]
    fn the_ratio_of_ninety_nine_percent_round_trips() {
        let volume = Percent::clamped(99);
        assert_eq!(Percent::from_ratio(volume.ratio()), volume);
    }
}
