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
    use proptest::prelude::{any, prop_assert, proptest};
    use rstest::rstest;

    use crate::primitive::time_text::{
        duration_text,
        elapsed_text,
        elapsed_width,
        relative_time_text,
    };

    #[rstest]
    #[case(0, "00:00")]
    #[case(65, "01:05")]
    #[case(3599, "59:59")]
    #[case(3600, "1:00:00")]
    #[case(3661, "1:01:01")]
    #[case(7384, "2:03:04")]
    fn duration_text_grows_an_hours_field_only_when_there_is_one(
        #[case] seconds: u64,
        #[case] expected: &str,
    ) {
        assert_eq!(duration_text(Duration::from_secs(seconds)), expected);
    }

    #[rstest]
    #[case::zero(0, 0)]
    #[case::the_last_minute_of_the_first_hour(3_599, 3_599)]
    #[case::the_first_hour(3_600, 3_600)]
    #[case::ten_hours(36_000, 36_000)]
    #[case::mixed(0, 36_000)]
    #[case::mixed_the_other_way(36_000, 3_599)]
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
        let moment = |seconds| Moment::new(Duration::from_secs(seconds));
        assert_eq!(relative_time_text(moment(now), moment(then)), text);
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
            let moment = |seconds| Moment::new(Duration::from_secs(seconds));
            let then_earlier = now_seconds.saturating_sub(earlier_offset);
            let then_later = then_earlier.saturating_add(gap).min(now_seconds);
            let earlier_label = relative_time_text(moment(now_seconds), moment(then_earlier));
            let later_label = relative_time_text(moment(now_seconds), moment(then_later));
            prop_assert!(rank(&later_label) <= rank(&earlier_label));
        }
    }
}
