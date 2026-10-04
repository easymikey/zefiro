use std::time::Duration;

use kernel::domain::time::Moment;

const JUST_NOW_SECONDS: u64 = 60;
const MINUTE_SECONDS: u64 = 60;
const HOUR_SECONDS: u64 = 3_600;
const DAY_SECONDS: u64 = 86_400;
const WEEK_SECONDS: u64 = 604_800;

#[must_use]
pub(crate) fn format_time(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let (hours, minutes, seconds) = (
        total_secs / HOUR_SECONDS,
        (total_secs % HOUR_SECONDS) / MINUTE_SECONDS,
        total_secs % MINUTE_SECONDS,
    );
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[must_use]
pub(crate) fn elapsed_of(position: Duration, duration: Duration) -> String {
    format!("{} / {}", format_time(position), format_time(duration))
}

#[must_use]
pub(crate) fn relative_time(now: Moment, then: Moment) -> String {
    let elapsed = now.elapsed_since(then).as_secs();
    if elapsed < JUST_NOW_SECONDS {
        return "just now".to_string();
    }
    if elapsed < HOUR_SECONDS {
        return format!("{}m ago", elapsed / MINUTE_SECONDS);
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
    use proptest::prelude::{any, prop_assert, proptest};
    use rstest::rstest;

    use crate::primitive::relative_time::{format_time, relative_time};

    #[rstest]
    #[case(0, "00:00")]
    #[case(65, "01:05")]
    #[case(3599, "59:59")]
    #[case(3600, "1:00:00")]
    #[case(3661, "1:01:01")]
    #[case(7384, "2:03:04")]
    fn format_time_grows_an_hours_field_only_when_there_is_one(
        #[case] seconds: u64,
        #[case] expected: &str,
    ) {
        assert_eq!(format_time(Duration::from_secs(seconds)), expected);
    }

    #[rstest]
    #[case::under_a_minute(59, 0, "just now")]
    #[case::minutes(15 * 60, 0, "15m ago")]
    #[case::exactly_one_hour(3_600, 0, "1h ago")]
    #[case::hours(3 * 3_600, 0, "3h ago")]
    #[case::days(2 * 86_400, 0, "2d ago")]
    #[case::weeks(3 * 604_800, 0, "3w ago")]
    #[case::in_the_future(0, 100, "just now")]
    fn relative_time_reports_the_coarsest_unit_that_fits(
        #[case] now: u64,
        #[case] then: u64,
        #[case] text: &str,
    ) {
        let at = |seconds| Moment::new(Duration::from_secs(seconds));
        assert_eq!(relative_time(at(now), at(then)), text);
    }

    fn rank(label: &str) -> (u8, u64) {
        if label == "just now" {
            return (0, 0);
        }
        let core = label.strip_suffix(" ago").unwrap_or(label);
        let unit = core.chars().last().unwrap_or('?');
        let digits = core.strip_suffix(unit).unwrap_or(core);
        let number: u64 = digits.parse().unwrap_or(0);
        let class = match unit {
            'm' => 1,
            'h' => 2,
            'd' => 3,
            'w' => 4,
            _ => 5,
        };

        (class, number)
    }

    proptest! {
        #[test]
        fn relative_time_is_monotonic_in_then(
            now_seconds in any::<u64>(),
            earlier_offset in 0u64..2_000_000,
            gap in 0u64..2_000_000,
        ) {
            let at = |seconds| Moment::new(Duration::from_secs(seconds));
            let then_earlier = now_seconds.saturating_sub(earlier_offset);
            let then_later = then_earlier.saturating_add(gap).min(now_seconds);
            let earlier_label = relative_time(at(now_seconds), at(then_earlier));
            let later_label = relative_time(at(now_seconds), at(then_later));
            prop_assert!(rank(&later_label) <= rank(&earlier_label));
        }
    }
}
