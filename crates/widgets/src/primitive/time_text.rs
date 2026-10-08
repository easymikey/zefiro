use std::time::Duration;

use kernel::domain::time::{Moment, SECONDS_PER_MINUTE};

use crate::pixels::numeric::small_count_u16;

const JUST_NOW_SECONDS: u64 = 60;
const HOUR_SECONDS: u64 = 3_600;
const DAY_SECONDS: u64 = 86_400;
const WEEK_SECONDS: u64 = 604_800;

#[must_use]
pub(crate) fn duration_text(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let (hours, minutes, seconds) = (
        total_secs / HOUR_SECONDS,
        (total_secs % HOUR_SECONDS) / SECONDS_PER_MINUTE,
        total_secs % SECONDS_PER_MINUTE,
    );
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[must_use]
pub(crate) fn elapsed_text(position: Duration, duration: Duration) -> String {
    format!("{} / {}", duration_text(position), duration_text(duration))
}

#[must_use]
pub(crate) fn elapsed_width(position: Duration, duration: Duration) -> u16 {
    [position, duration]
        .into_iter()
        .map(|time| {
            let hours = time.as_secs() / HOUR_SECONDS;
            small_count_u16("00:00".len())
                + hours.checked_ilog10().map_or(0, |tens| {
                    small_count_u16(tens + 1) + small_count_u16(":".len())
                })
        })
        .sum::<u16>()
        + small_count_u16(" / ".len())
}

#[must_use]
pub(crate) fn relative_time_text(now: Moment, then_at: Moment) -> String {
    let elapsed = now.elapsed_since(then_at).as_secs();
    if elapsed < JUST_NOW_SECONDS {
        return "just now".to_string();
    }
    if elapsed < HOUR_SECONDS {
        return format!("{}m ago", elapsed / SECONDS_PER_MINUTE);
    }
    if elapsed < DAY_SECONDS {
        return format!("{}h ago", elapsed / HOUR_SECONDS);
    }
    if elapsed < WEEK_SECONDS {
        return format!("{}d ago", elapsed / DAY_SECONDS);
    }
    format!("{}w ago", elapsed / WEEK_SECONDS)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::time::Moment;
    use rstest::rstest;

    use crate::primitive::time_text::{
        duration_text,
        elapsed_text,
        elapsed_width,
        relative_time_text,
    };

    #[rstest]
    #[case(65, "01:05")]
    #[case(3599, "59:59")]
    #[case(3600, "1:00:00")]
    #[case(7384, "2:03:04")]
    fn duration_text_grows_an_hours_field_only_when_there_is_one(
        #[case] seconds: u64,
        #[case] expected: &str,
    ) {
        assert_eq!(duration_text(Duration::from_secs(seconds)), expected);
    }

    #[rstest]
    #[case::the_last_minute_of_the_first_hour(3_599, 3_599)]
    #[case::ten_hours(36_000, 36_000)]
    #[case::mixed(0, 36_000)]
    fn elapsed_width_is_the_width_of_the_elapsed_text(
        #[case] position_seconds: u64,
        #[case] duration_seconds: u64,
    ) {
        let position = Duration::from_secs(position_seconds);
        let duration = Duration::from_secs(duration_seconds);
        assert_eq!(
            usize::from(elapsed_width(position, duration)),
            elapsed_text(position, duration).chars().count()
        );
    }

    #[rstest]
    #[case::under_a_minute(59, 0, "just now")]
    #[case::exactly_one_minute(60, 0, "1m ago")]
    #[case::minutes(15 * 60, 0, "15m ago")]
    #[case::exactly_one_hour(3_600, 0, "1h ago")]
    #[case::exactly_one_day(86_400, 0, "1d ago")]
    #[case::days(2 * 86_400, 0, "2d ago")]
    #[case::exactly_one_week(604_800, 0, "1w ago")]
    #[case::weeks(3 * 604_800, 0, "3w ago")]
    #[case::in_the_future(0, 100, "just now")]
    fn relative_time_reports_the_coarsest_unit_that_fits(
        #[case] now: u64,
        #[case] then: u64,
        #[case] text: &str,
    ) {
        let moment = |seconds| Moment::new(Duration::from_secs(seconds));
        assert_eq!(relative_time_text(moment(now), moment(then)), text);
    }
}
