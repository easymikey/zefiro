use std::time::Duration;

use crate::domain::{speed::Speed, time::Moment};

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    pub offset: Duration,
    pub started_at: Moment,
    pub speed: Speed,
}

impl Playhead {
    pub fn anchored(offset: Duration, started_at: Moment, speed: Speed) -> Self {
        Self {
            offset,
            started_at,
            speed,
        }
    }

    #[must_use]
    pub fn position_at(self, now: Moment) -> Duration {
        let elapsed = now.elapsed_since(self.started_at);
        self.offset + elapsed.mul_f32(self.speed.get())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::{
        bounded::Bounded,
        playhead::Playhead,
        speed::Speed,
        time::Moment,
    };

    #[rstest]
    #[case::no_time_passed_keeps_the_offset(1.0, 0, 10_000)]
    #[case::a_second_at_unity_speed_advances_a_second(1.0, 1, 11_000)]
    #[case::a_second_at_double_speed_advances_two_seconds(2.0, 1, 12_000)]
    #[case::a_second_at_half_speed_advances_half_a_second(0.5, 1, 10_500)]
    fn position_at_scales_elapsed_time_by_speed(
        #[case] speed_factor: f32,
        #[case] elapsed_secs: u64,
        #[case] expected_millis: u64,
    ) {
        let playhead = Playhead::anchored(
            Duration::from_secs(10),
            Moment::new(Duration::ZERO),
            Speed::clamped(speed_factor),
        );
        let now = Moment::new(Duration::from_secs(elapsed_secs));
        assert_eq!(
            playhead.position_at(now),
            Duration::from_millis(expected_millis)
        );
    }
}
