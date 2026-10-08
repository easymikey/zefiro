use std::time::Duration;

use kernel::domain::{
    bounded::Bounded,
    playhead::Playhead,
    speed::Speed,
    time::Moment,
};
use rstest::rstest;
use widgets::repaint::next_clock_second;

fn playhead(offset_secs: f64, speed_factor: f32) -> Playhead {
    Playhead::anchored(
        Duration::from_secs_f64(offset_secs),
        Moment::default(),
        Speed::clamped(speed_factor),
    )
}

#[rstest]
#[case::whole_second(10.0, 1.0, 1001)]
#[case::partial_second(10.4, 1.0, 601)]
#[case::double_speed(10.4, 2.0, 301)]
#[case::quarter_speed(10.4, 0.25, 2401)]
fn the_next_clock_second_lands_on_the_following_whole_second_at_any_speed(
    #[case] offset_secs: f64,
    #[case] speed_factor: f32,
    #[case] expected_millis: u64,
) {
    let playhead = playhead(offset_secs, speed_factor);
    let now = Moment::default();
    let result = next_clock_second(playhead, now);
    let millis = u64::try_from(result.since_epoch().as_millis()).unwrap_or(u64::MAX);
    assert_eq!(millis, expected_millis);
}
