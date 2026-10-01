use std::time::Duration;

use kernel::domain::{Bounded, Crossfade, Percent, SleepPresets, Speed};
use proptest::prelude::{any, prop_assert, proptest};

use crate::support::strategies::durations;

proptest! {
    #[test]
    fn percent_clamped_stays_within_bounds(raw in any::<u8>()) {
        prop_assert!(Percent::clamped(raw).get() <= Percent::MAX);
    }

    #[test]
    fn crossfade_clamped_stays_within_bounds(millis in 0u64..20_000) {
        let clamped = Crossfade::clamped(Duration::from_millis(millis)).get();
        prop_assert!(clamped >= Crossfade::MIN);
        prop_assert!(clamped <= Crossfade::MAX);
    }

    #[test]
    fn speed_clamped_stays_within_bounds(raw in any::<f32>()) {
        let clamped = Speed::clamped(raw).get();
        prop_assert!(clamped >= Speed::MIN);
        prop_assert!(clamped <= Speed::MAX);
    }

    #[test]
    fn speed_clamped_never_produces_nan(raw in any::<f32>()) {
        prop_assert!(!Speed::clamped(raw).get().is_nan());
    }

    #[test]
    fn nearest_bundle_is_in_range(current in durations(0..4)) {
        prop_assert!(SleepPresets::nearest_bundle(&current) < SleepPresets::BUNDLES.len());
    }
}
