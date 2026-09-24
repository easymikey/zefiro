use std::time::Duration;

use kernel::domain::format_time;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RelativeTimeThresholds {
    pub(crate) just_now_seconds: u64,
    pub(crate) minute_seconds: u64,
    pub(crate) hour_seconds: u64,
    pub(crate) day_seconds: u64,
    pub(crate) week_seconds: u64,
}

impl Default for RelativeTimeThresholds {
    fn default() -> Self {
        Self {
            just_now_seconds: 60,
            minute_seconds: 60,
            hour_seconds: 3_600,
            day_seconds: 86_400,
            week_seconds: 604_800,
        }
    }
}

#[must_use]
pub(crate) fn elapsed_of(position: Duration, duration: Duration) -> String {
    format!("{} / {}", format_time(position), format_time(duration))
}

#[must_use]
pub(crate) fn relative_time(now_unix: u64, then_unix: u64) -> String {
    let thresholds = RelativeTimeThresholds::default();
    let elapsed = now_unix.saturating_sub(then_unix);
    if elapsed < thresholds.just_now_seconds {
        return "just now".to_string();
    }
    if elapsed < thresholds.hour_seconds {
        return format!("{}m ago", elapsed / thresholds.minute_seconds);
    }
    if elapsed < thresholds.day_seconds {
        return format!("{}h ago", elapsed / thresholds.hour_seconds);
    }
    if elapsed < thresholds.week_seconds {
        return format!("{}d ago", elapsed / thresholds.day_seconds);
    }
    format!("{}w ago", elapsed / thresholds.week_seconds)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::{any, prop_assert, proptest};
    use rstest::rstest;

    use crate::primitive::relative_time::relative_time;

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
        assert_eq!(relative_time(now, then), text);
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
            now_unix in any::<u64>(),
            earlier_offset in 0u64..2_000_000,
            gap in 0u64..2_000_000,
        ) {
            let then_earlier = now_unix.saturating_sub(earlier_offset);
            let then_later = then_earlier.saturating_add(gap).min(now_unix);
            let earlier_label = relative_time(now_unix, then_earlier);
            let later_label = relative_time(now_unix, then_later);
            prop_assert!(rank(&later_label) <= rank(&earlier_label));
        }
    }
}
