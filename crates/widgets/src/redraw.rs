use std::{num::NonZeroU32, time::Duration};

use kernel::{Moment, Playhead};

const STEP_CORRECTION: Duration = Duration::from_millis(1);
const SECONDS_PER_MINUTE: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressScale {
    pub steps: NonZeroU32,
    pub length: Duration,
}

impl ProgressScale {
    #[must_use]
    pub fn text_bar(width: u16, length: Duration) -> Option<Self> {
        if width == 0 || length.is_zero() {
            return None;
        }
        let steps = NonZeroU32::new(2 * u32::from(width))?;
        Some(Self { steps, length })
    }
}

#[must_use]
pub fn next_progress_step(
    scale: ProgressScale,
    playhead: Playhead,
    now: Moment,
) -> Option<Moment> {
    let step_length = scale.length / scale.steps.get();
    if step_length.is_zero() {
        return None;
    }
    let position = playhead.position_at(now);
    let step_index = duration_steps(position, step_length);
    let boundary = step_length.saturating_mul(step_index.saturating_add(1));
    if boundary > scale.length {
        return None;
    }
    Some(wall_moment(playhead, boundary))
}

#[must_use]
pub fn next_clock_second(playhead: Playhead, now: Moment) -> Moment {
    let position = playhead.position_at(now);
    let boundary = Duration::from_secs(position.as_secs() + 1);
    wall_moment(playhead, boundary)
}

#[must_use]
pub fn next_sleep_minute(deadline: Moment, now: Moment) -> Option<Moment> {
    if now >= deadline {
        return None;
    }
    let left = deadline.elapsed_since(now);
    let minutes = ceil_minutes(left);
    let boundary = Duration::from_secs((minutes - 1) * SECONDS_PER_MINUTE);
    Some(Moment::new(deadline.since_epoch().saturating_sub(boundary)))
}

pub(crate) fn ceil_minutes(duration: Duration) -> u64 {
    let seconds = duration.as_secs() + u64::from(duration.subsec_nanos() > 0);
    seconds.div_ceil(SECONDS_PER_MINUTE)
}

fn duration_steps(position: Duration, step_length: Duration) -> u32 {
    let steps = position.as_nanos() / step_length.as_nanos();
    u32::try_from(steps).unwrap_or(u32::MAX)
}

fn wall_moment(playhead: Playhead, target_position: Duration) -> Moment {
    let delta = target_position.saturating_sub(playhead.offset);
    let wall_delta = delta.div_f32(playhead.speed.value()) + STEP_CORRECTION;
    Moment::new(playhead.since.since_epoch() + wall_delta)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::Moment;
    use rstest::rstest;

    use crate::redraw::next_sleep_minute;

    #[rstest]
    #[case::minute_boundary_soon(Duration::from_secs(14 * 60 + 59), Duration::from_secs(59))]
    #[case::exact_quarter_hour(Duration::from_secs(15 * 60), Duration::from_secs(60))]
    #[case::deadline_itself(Duration::from_secs(30), Duration::from_secs(30))]
    fn next_sleep_minute_rows(#[case] left: Duration, #[case] until_next: Duration) {
        let now = Moment::new(Duration::from_secs(1_000));
        let deadline = Moment::new(now.since_epoch() + left);
        assert_eq!(
            next_sleep_minute(deadline, now),
            Some(Moment::new(now.since_epoch() + until_next))
        );
    }

    #[test]
    fn next_sleep_minute_is_none_at_or_after_the_deadline() {
        let now = Moment::new(Duration::from_secs(1_000));
        assert_eq!(next_sleep_minute(now, now), None);
        assert_eq!(
            next_sleep_minute(Moment::new(Duration::from_secs(999)), now),
            None
        );
    }
}
