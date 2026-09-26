use std::{num::NonZeroU32, time::Duration};

use kernel::{Bounded, Moment, Playhead, Speed};
use rstest::rstest;
use widgets::{ProgressScale, next_clock_second, next_progress_step};

fn playhead(offset_secs: f64, speed: f32) -> Playhead {
    Playhead::anchored(
        Duration::from_secs_f64(offset_secs),
        Moment::default(),
        Speed::clamped(speed),
    )
}

fn hundred_steps() -> ProgressScale {
    ProgressScale {
        steps: NonZeroU32::new(100).unwrap_or(NonZeroU32::MIN),
        length: Duration::from_secs(100),
    }
}

#[rstest]
#[case::unity_speed(10.2, 1.0, Some(801))]
#[case::double_speed(10.2, 2.0, Some(401))]
#[case::quarter_speed(10.2, 0.25, Some(3201))]
#[case::exactly_on_a_boundary(10.0, 1.0, Some(1001))]
#[case::last_step(99.5, 1.0, Some(501))]
#[case::past_the_end(100.0, 1.0, None)]
fn next_progress_step_lands_on_the_next_boundary(
    #[case] offset_secs: f64,
    #[case] speed: f32,
    #[case] expected_millis: Option<u64>,
) {
    let head = playhead(offset_secs, speed);
    let now = Moment::default();
    let result = next_progress_step(hundred_steps(), head, now);
    let millis = result.map(|moment| {
        u64::try_from(moment.since_epoch().as_millis()).unwrap_or(u64::MAX)
    });
    assert_eq!(millis, expected_millis);
}

#[rstest]
#[case::quarter_speed(0.25)]
#[case::half_speed(0.5)]
#[case::unity_speed(1.0)]
#[case::one_and_a_half_speed(1.5)]
#[case::double_speed(2.0)]
#[case::quadruple_speed(4.0)]
fn the_returned_moment_really_crosses(#[case] speed: f32) {
    let scale = hundred_steps();
    let head = playhead(7.37, speed);
    let now = Moment::default();
    let step_before = head.position_at(now).as_secs();
    let result = next_progress_step(scale, head, now).unwrap_or(now);
    assert!(result > now, "expected a moment strictly after now");
    let step_after = head.position_at(result).as_secs();
    assert_eq!(step_after, step_before + 1);
}

#[rstest]
#[case::whole_second(10.0, 1.0, 1001)]
#[case::partial_second(10.4, 1.0, 601)]
#[case::double_speed(10.4, 2.0, 301)]
#[case::quarter_speed(10.4, 0.25, 2401)]
fn next_clock_second_rows(
    #[case] offset_secs: f64,
    #[case] speed: f32,
    #[case] expected_millis: u64,
) {
    let head = playhead(offset_secs, speed);
    let now = Moment::default();
    let result = next_clock_second(head, now);
    let millis = u64::try_from(result.since_epoch().as_millis()).unwrap_or(u64::MAX);
    assert_eq!(millis, expected_millis);
}

#[rstest]
#[case::zero_width(0, 100, None)]
#[case::zero_length(40, 0, None)]
#[case::forty_columns(40, 100, Some(80))]
fn text_bar_scale_rows(
    #[case] width: u16,
    #[case] length_secs: u64,
    #[case] expected_steps: Option<u32>,
) {
    let scale = ProgressScale::text_bar(width, Duration::from_secs(length_secs));
    assert_eq!(scale.map(|scale| scale.steps.get()), expected_steps);
}
