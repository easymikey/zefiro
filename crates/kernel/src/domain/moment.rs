use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Moment(Duration);

impl Moment {
    #[must_use]
    pub fn new(since_epoch: Duration) -> Self {
        Self(since_epoch)
    }

    #[must_use]
    pub fn since_epoch(self) -> Duration {
        self.0
    }

    #[must_use]
    pub fn elapsed_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixSeconds(i64);

impl UnixSeconds {
    pub const UNSTAMPED: Self = Self(0);

    #[must_use]
    pub fn of(moment: Moment) -> Self {
        Self(i64::try_from(moment.since_epoch().as_secs()).unwrap_or(i64::MAX))
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::moment::{Moment, UnixSeconds};

    #[test]
    fn since_epoch_returns_the_stored_duration() {
        let moment = Moment::new(Duration::from_secs(3));
        assert_eq!(moment.since_epoch(), Duration::from_secs(3));
    }

    #[test]
    fn default_is_the_epoch_itself() {
        assert_eq!(Moment::default(), Moment::new(Duration::ZERO));
    }

    #[test]
    fn elapsed_since_is_the_gap_between_two_moments() {
        let earlier = Moment::new(Duration::from_secs(3));
        let later = Moment::new(Duration::from_secs(5));
        assert_eq!(later.elapsed_since(earlier), Duration::from_secs(2));
    }

    #[test]
    fn elapsed_since_saturates_when_the_other_moment_is_later() {
        let earlier = Moment::new(Duration::from_secs(3));
        let later = Moment::new(Duration::from_secs(5));
        assert_eq!(earlier.elapsed_since(later), Duration::ZERO);
    }

    #[rstest]
    #[case::epoch(Duration::ZERO, 0)]
    #[case::truncates_the_fractional_second(Duration::from_millis(1900), 1)]
    fn unix_seconds_of_a_moment(#[case] since_epoch: Duration, #[case] expected: i64) {
        assert_eq!(UnixSeconds::of(Moment::new(since_epoch)).get(), expected);
    }
}
