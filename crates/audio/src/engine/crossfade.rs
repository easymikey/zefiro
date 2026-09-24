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
pub(crate) fn fade_fraction(position: Duration, crossfade: Duration) -> f32 {
    if crossfade.is_zero() {
        return 1.0;
    }
    (position.as_secs_f32() / crossfade.as_secs_f32()).clamp(0.0, 1.0)
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CrossfadeAction {
    Wait,
    Fade(f32),
    Handoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SinkDrain {
    Drained,
    Playing,
}

#[derive(Debug)]
pub(crate) struct CrossfadeMoment {
    pub(crate) sink_drained: SinkDrain,
    pub(crate) total: Option<Duration>,
    pub(crate) position: Duration,
    pub(crate) crossfade: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Promotion {
    Preload,
    Nothing,
}

#[must_use]
pub(crate) fn promotion_on_abandon(fade: Fade) -> Promotion {
    if matches!(fade, Fade::Fading(fraction) if fraction >= 0.5) {
        Promotion::Preload
    } else {
        Promotion::Nothing
    }
}

#[must_use]
pub(crate) fn crossfade_action(moment: &CrossfadeMoment) -> CrossfadeAction {
    let CrossfadeMoment {
        sink_drained,
        total,
        position,
        crossfade,
    } = *moment;
    if matches!(sink_drained, SinkDrain::Drained) {
        return CrossfadeAction::Handoff;
    }
    let fade_start = match total {
        Some(total) if !total.is_zero() => total.saturating_sub(crossfade),
        Some(_) | None => return CrossfadeAction::Wait,
    };
    if position < fade_start {
        return CrossfadeAction::Wait;
    }
    let fraction = fade_fraction(position.saturating_sub(fade_start), crossfade);
    if fraction >= 1.0 {
        CrossfadeAction::Handoff
    } else {
        CrossfadeAction::Fade(fraction)
    }
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
                CrossfadeAction,
                CrossfadeMoment,
                Promotion,
                SinkDrain,
                crossfade_action,
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

    struct CrossfadeRow {
        sink_drained: SinkDrain,
        total_secs: Option<u64>,
        position_secs: u64,
        crossfade_secs: u64,
        expected: CrossfadeAction,
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

    fn rounded(action: CrossfadeAction) -> CrossfadeAction {
        match action {
            CrossfadeAction::Fade(fraction) => {
                CrossfadeAction::Fade((fraction * 10_000.0).round() / 10_000.0)
            }
            CrossfadeAction::Wait => CrossfadeAction::Wait,
            CrossfadeAction::Handoff => CrossfadeAction::Handoff,
        }
    }

    #[rstest]
    #[case::waits_before_the_window(CrossfadeRow {
        sink_drained: SinkDrain::Playing,
        total_secs: Some(100),
        position_secs: 50,
        crossfade_secs: 5,
        expected: CrossfadeAction::Wait,
    })]
    #[case::waits_while_the_total_is_unknown(CrossfadeRow {
        sink_drained: SinkDrain::Playing,
        total_secs: None,
        position_secs: 5,
        crossfade_secs: 3,
        expected: CrossfadeAction::Wait,
    })]
    #[case::hands_off_when_a_sink_with_no_total_drains(CrossfadeRow {
        sink_drained: SinkDrain::Drained,
        total_secs: None,
        position_secs: 5,
        crossfade_secs: 3,
        expected: CrossfadeAction::Handoff,
    })]
    #[case::hands_off_when_a_sink_drains_early(CrossfadeRow {
        sink_drained: SinkDrain::Drained,
        total_secs: Some(100),
        position_secs: 100,
        crossfade_secs: 3,
        expected: CrossfadeAction::Handoff,
    })]
    #[case::interpolates_inside_the_window(CrossfadeRow {
        sink_drained: SinkDrain::Playing,
        total_secs: Some(100),
        position_secs: 92,
        crossfade_secs: 10,
        expected: CrossfadeAction::Fade(0.2),
    })]
    #[case::hands_off_at_the_end(CrossfadeRow {
        sink_drained: SinkDrain::Playing,
        total_secs: Some(100),
        position_secs: 100,
        crossfade_secs: 10,
        expected: CrossfadeAction::Handoff,
    })]
    #[case::waits_after_a_backward_seek(CrossfadeRow {
        sink_drained: SinkDrain::Playing,
        total_secs: Some(100),
        position_secs: 80,
        crossfade_secs: 10,
        expected: CrossfadeAction::Wait,
    })]
    #[case::opens_on_time_past_the_preload_lead(CrossfadeRow {
        sink_drained: SinkDrain::Playing,
        total_secs: Some(100),
        position_secs: 90,
        crossfade_secs: 20,
        expected: CrossfadeAction::Fade(0.5),
    })]
    fn crossfade_action_rows(#[case] row: CrossfadeRow) {
        let action = crossfade_action(&CrossfadeMoment {
            sink_drained: row.sink_drained,
            total: row.total_secs.map(Duration::from_secs),
            position: Duration::from_secs(row.position_secs),
            crossfade: Duration::from_secs(row.crossfade_secs),
        });
        assert_eq!(rounded(action), row.expected);
    }

    #[rstest]
    #[case::never_started(Fade::Idle, Promotion::Nothing)]
    #[case::just_started(Fade::Fading(0.0), Promotion::Nothing)]
    #[case::below_the_midpoint(Fade::Fading(0.2), Promotion::Nothing)]
    #[case::just_below_the_midpoint(Fade::Fading(0.49), Promotion::Nothing)]
    #[case::at_the_midpoint(Fade::Fading(0.5), Promotion::Preload)]
    #[case::past_the_midpoint(Fade::Fading(0.9), Promotion::Preload)]
    #[case::complete(Fade::Fading(1.0), Promotion::Preload)]
    fn abandoning_a_crossfade_promotes_only_the_louder_preload(
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

    proptest! {
        #[test]
        fn equal_power_gains_sum_of_squares_to_one_across_the_fade(fraction in 0f32..=1f32) {
            let power = gain_in(fraction)
                .mul_add(gain_in(fraction), gain_out(fraction) * gain_out(fraction));
            prop_assert!((power - 1.0).abs() < 1e-4, "expected equal power, got {power}");
        }
    }
}
