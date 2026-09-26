use std::{f32::consts::FRAC_PI_2, time::Duration};

use kernel::domain::Replaygain;

use crate::{EngineConfig, UnityVolume, engine::phase::Fade};

#[must_use]
pub(crate) fn gain_in(fraction: f32) -> f32 {
    (fraction.clamp(0.0, 1.0) * FRAC_PI_2).sin()
}

#[must_use]
pub(crate) fn gain_out(fraction: f32) -> f32 {
    (fraction.clamp(0.0, 1.0) * FRAC_PI_2).cos()
}

#[must_use]
pub(crate) fn effective_volume(
    config: &EngineConfig,
    gain_db: Option<f32>,
    user_factor: f32,
) -> f32 {
    let user = if matches!(config.unity_volume, UnityVolume::Pinned) {
        1.0
    } else {
        user_factor
    };
    let gain = if matches!(config.replaygain, Replaygain::On) {
        gain_db.map_or(1.0, |g| 10f32.powf(g / 20.0))
    } else {
        1.0
    };
    user * gain
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Promotion {
    Preload,
    Nothing,
}

#[must_use]
pub(crate) fn promotion_on_abandon(fade: Fade) -> Promotion {
    match fade {
        Fade::Fading => Promotion::Preload,
        Fade::Idle => Promotion::Nothing,
    }
}

#[must_use]
pub(crate) fn arm_cue(
    total: Option<Duration>,
    crossfade: Duration,
) -> Option<Duration> {
    total.map(|total| total.saturating_sub(crossfade))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{Crossfade, Replaygain};
    use proptest::prelude::{prop_assert, proptest};
    use rstest::rstest;

    use crate::{
        EngineConfig,
        UnityVolume,
        engine::{
            crossfade::{
                Promotion,
                arm_cue,
                effective_volume,
                gain_in,
                gain_out,
                promotion_on_abandon,
            },
            phase::Fade,
        },
    };

    struct VolumeRow {
        replaygain: Replaygain,
        unity_volume: UnityVolume,
        gain: Option<f32>,
        user_factor: f32,
        expected: f32,
    }

    #[rstest]
    #[case::no_cached_gain(VolumeRow {
        replaygain: Replaygain::On,
        unity_volume: UnityVolume::Free,
        gain: None,
        user_factor: 0.5,
        expected: 0.5,
    })]
    #[case::replaygain_disabled_ignores_the_gain(VolumeRow {
        replaygain: Replaygain::Off,
        unity_volume: UnityVolume::Free,
        gain: Some(-6.0),
        user_factor: 1.0,
        expected: 1.0,
    })]
    #[case::replaygain_applies_decibels_as_a_linear_factor(VolumeRow {
        replaygain: Replaygain::On,
        unity_volume: UnityVolume::Free,
        gain: Some(-6.0),
        user_factor: 1.0,
        expected: 0.501_187,
    })]
    #[case::unity_volume_pins_a_quiet_user_factor(VolumeRow {
        replaygain: Replaygain::On,
        unity_volume: UnityVolume::Pinned,
        gain: Some(-6.0),
        user_factor: 0.1,
        expected: 0.501_187,
    })]
    #[case::unity_volume_pins_a_loud_user_factor(VolumeRow {
        replaygain: Replaygain::On,
        unity_volume: UnityVolume::Pinned,
        gain: Some(-6.0),
        user_factor: 0.9,
        expected: 0.501_187,
    })]
    fn effective_volume_combines_gain_and_user_factor(#[case] row: VolumeRow) {
        let engine_config = EngineConfig {
            crossfade: Crossfade::default(),
            replaygain: row.replaygain,
            unity_volume: row.unity_volume,
            device: None,
        };
        let factor = effective_volume(&engine_config, row.gain, row.user_factor);
        let expected = row.expected;
        assert!(
            (factor - expected).abs() < 1e-4,
            "expected ~{expected}, got {factor}"
        );
    }

    #[rstest]
    #[case::idle(Fade::Idle, Promotion::Nothing)]
    #[case::fading(Fade::Fading, Promotion::Preload)]
    fn abandoning_a_crossfade_promotes_only_a_fading_preload(
        #[case] fade: Fade,
        #[case] promotion: Promotion,
    ) {
        assert_eq!(promotion_on_abandon(fade), promotion);
    }

    #[rstest]
    #[case::silence_at_the_start(0.0, 0.0, 1.0)]
    #[case::equal_power_at_the_middle(0.5, 0.70710677, 0.70710677)]
    #[case::full_at_the_end(1.0, 1.0, 0.0)]
    fn the_equal_power_ramp_trades_one_gain_for_the_other(
        #[case] fraction: f32,
        #[case] incoming: f32,
        #[case] outgoing: f32,
    ) {
        assert!((gain_in(fraction) - incoming).abs() < 1e-5);
        assert!((gain_out(fraction) - outgoing).abs() < 1e-5);
        let power = gain_in(fraction)
            .mul_add(gain_in(fraction), gain_out(fraction) * gain_out(fraction));
        assert!((power - 1.0).abs() < 1e-5, "equal power, got {power}");
    }

    #[rstest]
    #[case::known_total(
        Some(Duration::from_secs(100)),
        Duration::from_secs(10),
        Some(Duration::from_secs(90))
    )]
    #[case::unknown_total(None, Duration::from_secs(10), None)]
    #[case::crossfade_past_the_total_saturates(
        Some(Duration::from_secs(5)),
        Duration::from_secs(10),
        Some(Duration::ZERO)
    )]
    fn the_cue_sits_a_crossfade_before_the_end(
        #[case] total: Option<Duration>,
        #[case] crossfade: Duration,
        #[case] expected: Option<Duration>,
    ) {
        assert_eq!(arm_cue(total, crossfade), expected);
    }

    proptest! {
        #[test]
        fn equal_power_gains_sum_of_squares_to_one_across_the_fade(fraction in 0f32..=1f32) {
            let power = gain_in(fraction)
                .mul_add(gain_in(fraction), gain_out(fraction) * gain_out(fraction));
            prop_assert!((power - 1.0).abs() < 1e-4, "expected equal power, got {power}");
        }
    }
}
