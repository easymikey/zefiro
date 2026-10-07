use std::time::Duration;

use crate::domain::time::SECONDS_PER_MINUTE;

const MAX_MINUTES: u64 = 720;
const MAX_PRESETS: usize = 5;

const fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * SECONDS_PER_MINUTE)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepPresets(Box<[Duration]>);

impl SleepPresets {
    pub const BUNDLES: &[&[Duration]] = &[
        &[minutes(15), minutes(30), minutes(60)],
        &[minutes(10), minutes(20), minutes(45)],
        &[minutes(30), minutes(60), minutes(90)],
        &[minutes(45), minutes(90), minutes(120)],
        &[],
    ];

    #[must_use]
    pub(crate) fn bundle_index(current: &[Duration]) -> Option<usize> {
        Self::BUNDLES.iter().position(|bundle| *bundle == current)
    }

    #[must_use]
    pub fn nearest_bundle(current: &[Duration]) -> usize {
        let current_duration: Duration = current.iter().sum();
        Self::BUNDLES
            .iter()
            .enumerate()
            .min_by_key(|(_, bundle)| {
                let duration: Duration = bundle.iter().sum();
                duration.abs_diff(current_duration)
            })
            .map_or(0, |(index, _)| index)
    }

    pub fn from_minutes(minutes: &[u64]) -> Result<Self, SleepPresetsError> {
        if minutes.len() > MAX_PRESETS {
            return Err(SleepPresetsError::TooMany(minutes.len()));
        }
        let mut previous = 0;
        for &value in minutes {
            if value == 0 || value > MAX_MINUTES {
                return Err(SleepPresetsError::OutOfRange {
                    duration: Duration::from_mins(value),
                    min: Duration::from_mins(1),
                    max: Duration::from_mins(MAX_MINUTES),
                });
            }
            if value <= previous {
                return Err(SleepPresetsError::NotAscending(Duration::from_mins(
                    value,
                )));
            }
            previous = value;
        }
        Ok(Self(
            minutes.iter().copied().map(Duration::from_mins).collect(),
        ))
    }

    #[must_use]
    pub fn bundle(bundle_index: usize) -> Self {
        Self(
            Self::BUNDLES
                .get(bundle_index % Self::BUNDLES.len())
                .copied()
                .unwrap_or(&[])
                .into(),
        )
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Duration] {
        &self.0
    }
}

impl Default for SleepPresets {
    fn default() -> Self {
        Self(SleepPresets::BUNDLES.first().copied().unwrap_or(&[]).into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SleepPresetsError {
    #[error("sleep preset of {} minutes is out of range (must be {}..={})", duration.as_secs() / SECONDS_PER_MINUTE, min.as_secs() / SECONDS_PER_MINUTE, max.as_secs() / SECONDS_PER_MINUTE)]
    OutOfRange {
        duration: Duration,
        min: Duration,
        max: Duration,
    },
    #[error("sleep preset of {} minutes is not above the one before", .0.as_secs() / SECONDS_PER_MINUTE)]
    NotAscending(Duration),
    #[error("at most 5 sleep presets are allowed, found {0}")]
    TooMany(usize),
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::{
        sleep_presets::{SleepPresets, SleepPresetsError},
        time::SECONDS_PER_MINUTE,
    };

    #[test]
    fn the_default_bundles_are_the_five_documented_ones() {
        let minutes = |bundle: &[Duration]| -> Vec<u64> {
            bundle
                .iter()
                .map(|preset| preset.as_secs() / SECONDS_PER_MINUTE)
                .collect()
        };
        assert_eq!(
            SleepPresets::BUNDLES
                .iter()
                .map(|b| minutes(b))
                .collect::<Vec<_>>(),
            vec![
                vec![15, 30, 60],
                vec![10, 20, 45],
                vec![30, 60, 90],
                vec![45, 90, 120],
                Vec::<u64>::new(),
            ]
        );
    }

    #[test]
    fn bundle_index_finds_an_exact_match_only() {
        let third = SleepPresets::BUNDLES.get(2).copied();
        assert_eq!(third.and_then(SleepPresets::bundle_index), Some(2));
        let custom = [Duration::from_secs(5 * 60)];
        assert_eq!(SleepPresets::bundle_index(&custom), None);
    }

    #[test]
    fn nearest_bundle_snaps_by_total_minutes() {
        let custom = [Duration::from_secs(100 * 60)];
        assert_eq!(SleepPresets::nearest_bundle(&custom), 0);
        assert_eq!(SleepPresets::nearest_bundle(&[]), 4);
    }

    #[rstest]
    #[case::empty_means_off(&[], vec![])]
    #[case::one_preset(&[15], vec![Duration::from_mins(15)])]
    #[case::three_presets(&[1, 360, 720], vec![Duration::from_mins(1), Duration::from_mins(360), Duration::from_mins(720)])]
    fn sleep_presets_from_minutes_accepts_a_valid_list(
        #[case] minutes: &[u64],
        #[case] expected: Vec<Duration>,
    ) {
        assert_eq!(
            SleepPresets::from_minutes(minutes).unwrap().as_slice(),
            expected.as_slice()
        );
    }

    #[rstest]
    #[case::zero_minutes(&[0], SleepPresetsError::OutOfRange { duration: Duration::ZERO, min: Duration::from_mins(1), max: Duration::from_mins(720) })]
    #[case::over_the_ceiling(&[721], SleepPresetsError::OutOfRange { duration: Duration::from_mins(721), min: Duration::from_mins(1), max: Duration::from_mins(720) })]
    #[case::not_ascending(&[30, 20], SleepPresetsError::NotAscending(Duration::from_mins(20)))]
    #[case::repeated(&[30, 30], SleepPresetsError::NotAscending(Duration::from_mins(30)))]
    #[case::too_many(&[1, 2, 3, 4, 5, 6], SleepPresetsError::TooMany(6))]
    fn sleep_presets_from_minutes_rejects_what_it_cannot_place(
        #[case] minutes: &[u64],
        #[case] expected: SleepPresetsError,
    ) {
        assert_eq!(SleepPresets::from_minutes(minutes), Err(expected));
    }

    #[test]
    fn sleep_presets_are_read_as_whole_minutes() {
        let presets = SleepPresets::from_minutes(&[10, 20]).unwrap();
        assert_eq!(
            presets.as_slice(),
            [Duration::from_mins(10), Duration::from_mins(20)]
        );
    }
}
