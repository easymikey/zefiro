use std::time::Duration;

use kernel::domain::{Bounded, Crossfade, Percent, SleepPresetBundles, Speed};
use proptest::prelude::{any, prop_assert, proptest};

use crate::support::strategies::durations;

proptest! {
    #[test]
    fn percent_clamped_stays_within_bounds(raw in any::<u8>()) {
        prop_assert!(Percent::clamped(raw).value() <= Percent::MAX);
    }

    #[test]
    fn crossfade_clamped_stays_within_bounds(millis in 0u64..20_000) {
        let clamped = Crossfade::clamped(Duration::from_millis(millis)).value();
        prop_assert!(clamped >= Crossfade::MIN);
        prop_assert!(clamped <= Crossfade::MAX);
    }

    #[test]
    fn speed_clamped_stays_within_bounds(raw in any::<f32>()) {
        let clamped = Speed::clamped(raw).value();
        prop_assert!(clamped >= Speed::MIN);
        prop_assert!(clamped <= Speed::MAX);
    }

    #[test]
    fn speed_clamped_never_produces_nan(raw in any::<f32>()) {
        prop_assert!(!Speed::clamped(raw).value().is_nan());
    }

    #[test]
    fn sleep_preset_bundles_nearest_index_is_in_range(
        bundles in proptest::collection::vec(durations(0..4), 1..6),
        current in durations(0..4),
    ) {
        let bundle_count = bundles.len();
        let presets = SleepPresetBundles { bundles };
        prop_assert!(presets.nearest_index(&current) < bundle_count);
    }
}

#[test]
fn a_new_bundle_closer_in_total_becomes_nearest() {
    let presets = SleepPresetBundles {
        bundles: vec![
            vec![Duration::from_secs(60 * 15)],
            vec![Duration::from_secs(60 * 90)],
        ],
    };
    assert_eq!(presets.nearest_index(&[Duration::from_secs(60 * 14)]), 0);
    assert_eq!(presets.nearest_index(&[Duration::from_secs(60 * 89)]), 1);
}
