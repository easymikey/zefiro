use std::fmt;

use crate::domain::bounded::Bounded;

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

    pub(crate) fn step_up(self) -> Self {
        Self::clamped(self.0 + Self::STEP)
    }

    pub(crate) fn step_down(self) -> Self {
        Self::clamped(self.0 - Self::STEP)
    }

    #[must_use]
    pub fn label(self) -> Option<String> {
        (self != Self::default()).then(|| self.trimmed_label())
    }

    fn trimmed_label(self) -> String {
        let formatted = format!("{:.2}", self.0);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self(1.0)
    }
}

impl fmt::Display for Speed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.trimmed_label())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{bounded::Bounded, speed::Speed};

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
    #[case::step_up_saturates_at_the_ceiling(Speed::clamped(4.0), Speed::step_up, 4.0)]
    #[case::step_down_saturates_at_the_floor(
        Speed::clamped(0.25),
        Speed::step_down,
        0.25
    )]
    #[case::step_up_from_default(Speed::default(), Speed::step_up, 1.25)]
    #[case::step_down_from_default(Speed::default(), Speed::step_down, 0.75)]
    fn step_saturates_at_bounds(
        #[case] start: Speed,
        #[case] step: fn(Speed) -> Speed,
        #[case] expected: f32,
    ) {
        assert_eq!(step(start).get(), expected);
    }

    #[test]
    fn label_hides_at_default() {
        assert_eq!(Speed::default().label(), None);
    }

    #[rstest]
    #[case::whole_number_trims_to_its_integer(2.0, "2")]
    #[case::one_trailing_zero_is_trimmed(0.5, "0.5")]
    #[case::two_significant_decimals_are_kept(1.25, "1.25")]
    fn label_formats_the_value(#[case] raw: f32, #[case] expected: &str) {
        assert_eq!(Speed::clamped(raw).label().as_deref(), Some(expected));
    }

    #[test]
    fn display_reports_the_default_as_one() {
        assert_eq!(Speed::default().to_string(), "1");
    }

    #[rstest]
    #[case::away_from_default(1.25, "1.25")]
    #[case::at_default(1.0, "1")]
    fn display_matches_trimmed_label(#[case] raw: f32, #[case] expected: &str) {
        assert_eq!(Speed::clamped(raw).to_string(), expected);
    }
}
