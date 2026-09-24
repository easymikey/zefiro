use std::{sync::LazyLock, time::Duration};

use crate::domain::time::SECONDS_PER_MINUTE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepPresetBundles {
    pub bundles: Vec<Vec<Duration>>,
}

fn minutes(values: &[u64]) -> Vec<Duration> {
    values
        .iter()
        .map(|value| Duration::from_secs(value * SECONDS_PER_MINUTE))
        .collect()
}

pub static SLEEP_PRESET_BUNDLES: LazyLock<SleepPresetBundles> =
    LazyLock::new(|| SleepPresetBundles {
        bundles: vec![
            minutes(&[15, 30, 60]),
            minutes(&[10, 20, 45]),
            minutes(&[30, 60, 90]),
            minutes(&[45, 90, 120]),
            Vec::new(),
        ],
    });

impl SleepPresetBundles {
    #[must_use]
    pub fn first(&self) -> Box<[Duration]> {
        self.bundles.first().cloned().unwrap_or_default().into()
    }

    #[must_use]
    pub fn index_of(&self, current: &[Duration]) -> Option<usize> {
        self.bundles
            .iter()
            .position(|bundle| bundle.as_slice() == current)
    }

    #[must_use]
    pub fn nearest_index(&self, current: &[Duration]) -> usize {
        let current_total: Duration = current.iter().sum();
        self.bundles
            .iter()
            .enumerate()
            .min_by_key(|(_, bundle)| {
                let total: Duration = bundle.iter().sum();
                duration_diff(total, current_total)
            })
            .map_or(0, |(index, _)| index)
    }
}

fn duration_diff(a: Duration, b: Duration) -> Duration {
    a.abs_diff(b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepTimer {
    pub preset_index: usize,
    pub delay: Duration,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::domain::sleep::SLEEP_PRESET_BUNDLES;

    #[test]
    fn sleep_preset_bundles_default_has_the_five_documented_bundles() {
        let bundles = &SLEEP_PRESET_BUNDLES;
        let minutes = |bundle: &[Duration]| -> Vec<u64> {
            bundle.iter().map(|preset| preset.as_secs() / 60).collect()
        };
        assert_eq!(
            bundles
                .bundles
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
    fn sleep_preset_bundles_index_of_finds_an_exact_match_only() {
        let bundles = &SLEEP_PRESET_BUNDLES;
        let third = bundles.bundles.get(2).map(Vec::as_slice);
        assert_eq!(third.and_then(|bundle| bundles.index_of(bundle)), Some(2));
        let custom = [Duration::from_secs(5 * 60)];
        assert_eq!(bundles.index_of(&custom), None);
    }

    #[test]
    fn sleep_preset_bundles_nearest_index_snaps_by_total_minutes() {
        let bundles = &SLEEP_PRESET_BUNDLES;
        let custom = [Duration::from_secs(100 * 60)];
        assert_eq!(bundles.nearest_index(&custom), 0);
        assert_eq!(bundles.nearest_index(&[]), 4);
    }
}
