use std::time::Duration;

use crate::domain::SLEEP_PRESET_BUNDLES;

const MAX_MINUTES: u64 = 720;
const MAX_PRESETS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepPresets(Box<[Duration]>);

impl SleepPresets {
    pub fn from_minutes(minutes: &[u64]) -> Result<Self, SleepPresetRejection> {
        if minutes.len() > MAX_PRESETS {
            return Err(SleepPresetRejection::TooMany {
                count: minutes.len(),
            });
        }
        let mut previous = 0;
        for &value in minutes {
            if value == 0 || value > MAX_MINUTES {
                return Err(SleepPresetRejection::OutOfRange { minutes: value });
            }
            if value <= previous {
                return Err(SleepPresetRejection::NotAscending { minutes: value });
            }
            previous = value;
        }
        Ok(Self(
            minutes.iter().copied().map(Duration::from_mins).collect(),
        ))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Duration] {
        &self.0
    }
}

impl Default for SleepPresets {
    fn default() -> Self {
        Self(SLEEP_PRESET_BUNDLES.first())
    }
}

impl From<SleepPresets> for Box<[Duration]> {
    fn from(presets: SleepPresets) -> Self {
        presets.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SleepPresetRejection {
    #[error("sleep preset of {minutes} minutes is out of range (must be 1..=720)")]
    OutOfRange { minutes: u64 },
    #[error(
        "sleep preset of {minutes} minutes does not come after the preset before it"
    )]
    NotAscending { minutes: u64 },
    #[error("at most 5 sleep presets are allowed, found {count}")]
    TooMany { count: usize },
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::sleep_presets::{SleepPresetRejection, SleepPresets};

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
    #[case::zero_minutes(&[0], SleepPresetRejection::OutOfRange { minutes: 0 })]
    #[case::over_the_ceiling(&[721], SleepPresetRejection::OutOfRange { minutes: 721 })]
    #[case::not_ascending(&[30, 20], SleepPresetRejection::NotAscending { minutes: 20 })]
    #[case::repeated(&[30, 30], SleepPresetRejection::NotAscending { minutes: 30 })]
    #[case::too_many(&[1, 2, 3, 4, 5, 6], SleepPresetRejection::TooMany { count: 6 })]
    fn sleep_presets_from_minutes_rejects_what_it_cannot_place(
        #[case] minutes: &[u64],
        #[case] expected: SleepPresetRejection,
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
