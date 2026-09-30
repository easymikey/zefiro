#![forbid(unsafe_code)]

use kernel::{Bounded, Percent};
use objc2_core_audio::AudioObjectID;

use crate::audio_hardware::{
    HardwareError,
    Muted,
    muted,
    set_muted,
    set_volume_scalar,
    volume_scalar,
};

pub(crate) fn read_volume(device: AudioObjectID) -> Option<Percent> {
    let scalar = volume_scalar(device)?;
    if matches!(muted(device), Some(true)) {
        Some(Percent::clamped(0))
    } else {
        Some(percent_from_scalar(scalar))
    }
}

pub(crate) fn write_volume(
    device: AudioObjectID,
    volume: Percent,
) -> Result<(), HardwareError> {
    set_volume_scalar(device, scalar_from_percent(volume))?;
    if let Some(target) = mute_target(device, volume) {
        let cleared = set_muted(device, target);
        if volume.value() > 0 {
            cleared?;
        }
    }
    Ok(())
}

fn mute_target(device: AudioObjectID, volume: Percent) -> Option<Muted> {
    match (volume.value() > 0, muted(device)) {
        (true, Some(true)) => Some(Muted::No),
        (false, Some(false)) => Some(Muted::Yes),
        (_, None) | (true, Some(false)) | (false, Some(true)) => None,
    }
}

fn percent_from_scalar(scalar: f32) -> Percent {
    let scalar = if scalar.is_nan() { 0.0 } else { scalar };
    let scaled = scalar.clamp(0.0, 1.0) * 100.0;
    let step = (0..=100u8)
        .find(|step| f32::from(*step) + 0.5 > scaled)
        .unwrap_or(100);
    Percent::clamped(step)
}

fn scalar_from_percent(volume: Percent) -> f32 {
    volume.ratio()
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, Percent};
    use rstest::rstest;

    use crate::{
        audio_hardware::default_output_device,
        volume::{percent_from_scalar, read_volume, scalar_from_percent, write_volume},
    };

    #[rstest]
    #[case::floor(0.0, 0)]
    #[case::rounds_down(0.404, 40)]
    #[case::rounds_up(0.406, 41)]
    #[case::ceiling(1.0, 100)]
    #[case::clamps_above_one(1.7, 100)]
    #[case::clamps_below_zero(-0.2, 0)]
    #[case::not_a_number_is_zero(f32::NAN, 0)]
    fn percent_from_scalar_rounds_and_clamps(#[case] scalar: f32, #[case] percent: u8) {
        assert_eq!(percent_from_scalar(scalar), Percent::clamped(percent));
    }

    #[rstest]
    #[case::silence(0)]
    #[case::a_sliver(1)]
    #[case::two_fifths(40)]
    #[case::almost_full(99)]
    #[case::full(100)]
    fn scalar_round_trips_every_percent(#[case] percent: u8) {
        let volume = Percent::clamped(percent);
        assert_eq!(percent_from_scalar(scalar_from_percent(volume)), volume);
    }

    #[test]
    #[ignore = "hardware: writes the system volume"]
    fn the_system_volume_reads_back_what_was_written() {
        let device = default_output_device();
        let original = read_volume(device);
        assert_eq!(write_volume(device, Percent::clamped(37)), Ok(()));
        let after = read_volume(device).unwrap();
        assert!(after.value().abs_diff(37) <= 1);
        if let Some(original) = original {
            assert_eq!(write_volume(device, original), Ok(()));
        }
    }
}
