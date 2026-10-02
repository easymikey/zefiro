use std::{f32::consts::FRAC_PI_2, time::Duration};

use kernel::domain::ReplayGain;

#[must_use]
pub(crate) fn gain_in(fraction: f32) -> f32 {
    (fraction.clamp(0.0, 1.0) * FRAC_PI_2).sin()
}

#[must_use]
pub(crate) fn gain_out(fraction: f32) -> f32 {
    (fraction.clamp(0.0, 1.0) * FRAC_PI_2).cos()
}

#[must_use]
pub(crate) fn replaygain_factor(replay_gain: ReplayGain, gain_db: Option<f32>) -> f32 {
    if matches!(replay_gain, ReplayGain::On) {
        gain_db.map_or(1.0, |g| 10f32.powf(g / 20.0))
    } else {
        1.0
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

    use kernel::domain::ReplayGain;
    use proptest::prelude::{prop_assert, proptest};
    use rstest::rstest;

    use crate::engine::crossfade::{arm_cue, gain_in, gain_out, replaygain_factor};

    struct VolumeRow {
        replay_gain: ReplayGain,
        gain: Option<f32>,
        expected: f32,
    }

    #[rstest]
    #[case::no_cached_gain(VolumeRow {
        replay_gain: ReplayGain::On,
        gain: None,
        expected: 1.0,
    })]
    #[case::replaygain_disabled_ignores_the_gain(VolumeRow {
        replay_gain: ReplayGain::Off,
        gain: Some(-6.0),
        expected: 1.0,
    })]
    #[case::replaygain_applies_decibels_as_a_linear_factor(VolumeRow {
        replay_gain: ReplayGain::On,
        gain: Some(-6.0),
        expected: 0.501_187,
    })]
    fn replaygain_factor_turns_decibels_into_a_linear_factor(#[case] row: VolumeRow) {
        let factor = replaygain_factor(row.replay_gain, row.gain);
        let expected = row.expected;
        assert!(
            (factor - expected).abs() < 1e-4,
            "expected ~{expected}, got {factor}"
        );
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
