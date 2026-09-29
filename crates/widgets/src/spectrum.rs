use std::time::Duration;

pub const SPECTRUM_BANDS: usize = 16;

pub type Spectrum = [f32; SPECTRUM_BANDS];

const REFERENCE_RATE_HZ: f32 = 60.0;
const SILENT_BAND: f32 = 1.0 / 1024.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectrumMotion {
    Moving,
    Settled,
}

#[derive(Debug, Clone, Copy)]
pub struct SpectrumSmoothing {
    attack: f32,
    decay: f32,
    bands: Spectrum,
}

impl Default for SpectrumSmoothing {
    fn default() -> Self {
        Self {
            attack: 0.85,
            decay: 0.35,
            bands: [0.0; SPECTRUM_BANDS],
        }
    }
}

fn effective_coefficient(base: f32, elapsed_secs: f32) -> f32 {
    (base * elapsed_secs * REFERENCE_RATE_HZ).min(1.0)
}

fn smooth_band(previous: f32, raw: f32, coefficient: f32) -> f32 {
    previous + (raw - previous) * coefficient
}

impl SpectrumSmoothing {
    #[must_use]
    pub fn smooth(&mut self, raw: &Spectrum, elapsed: Duration) -> Spectrum {
        let elapsed_secs = elapsed.as_secs_f32();
        let attack = effective_coefficient(self.attack, elapsed_secs);
        let decay = effective_coefficient(self.decay, elapsed_secs);
        for (previous, &level) in self.bands.iter_mut().zip(raw.iter()) {
            let coefficient = if level > *previous { attack } else { decay };
            let smoothed = smooth_band(*previous, level, coefficient);
            *previous = if smoothed.abs() < SILENT_BAND {
                0.0
            } else {
                smoothed
            };
        }
        self.bands
    }

    #[must_use]
    pub fn fade(&mut self, elapsed: Duration) -> Spectrum {
        self.smooth(&[0.0; SPECTRUM_BANDS], elapsed)
    }

    #[must_use]
    pub fn bands(&self) -> Spectrum {
        self.bands
    }

    #[must_use]
    pub fn current_bands(&self) -> &Spectrum {
        &self.bands
    }

    #[must_use]
    pub fn motion(&self) -> SpectrumMotion {
        if self.bands.iter().all(|&band| band == 0.0) {
            SpectrumMotion::Settled
        } else {
            SpectrumMotion::Moving
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::spectrum::{
        SILENT_BAND,
        SPECTRUM_BANDS,
        SpectrumMotion,
        SpectrumSmoothing,
    };

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn smoothing_moves_every_band_toward_the_raw_value_without_overshoot() {
        let mut smoothing = SpectrumSmoothing::default();
        let target = [1.0; SPECTRUM_BANDS];
        let mut previous = smoothing.bands();
        for _ in 0..5 {
            let bands = smoothing.smooth(&target, FRAME);
            for (band, &previous_band) in bands.iter().zip(previous.iter()) {
                assert!(*band > previous_band);
                assert!((0.0..=1.0).contains(band));
            }
            previous = bands;
        }
    }

    #[test]
    fn decay_pulls_bands_to_near_zero_within_a_few_frames() {
        let mut smoothing = SpectrumSmoothing::default();
        let _ = smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        let mut decayed = smoothing.bands();
        for _ in 0..30 {
            decayed = smoothing.fade(FRAME);
        }
        assert_eq!(decayed, [0.0; SPECTRUM_BANDS]);
    }

    #[test]
    fn bands_returns_the_same_values_without_advancing_them() {
        let mut smoothing = SpectrumSmoothing::default();
        let _ = smoothing.smooth(&[0.4; SPECTRUM_BANDS], FRAME);
        assert_eq!(smoothing.bands(), smoothing.bands());
    }

    #[test]
    fn a_longer_elapsed_time_moves_further_toward_the_raw_value() {
        let mut short = SpectrumSmoothing::default();
        let mut long = SpectrumSmoothing::default();
        let target = [1.0; SPECTRUM_BANDS];
        let short_bands = short.smooth(&target, Duration::from_millis(8));
        let long_bands = long.smooth(&target, Duration::from_millis(32));
        for (short_band, long_band) in short_bands.iter().zip(long_bands.iter()) {
            assert!(long_band > short_band);
        }
    }

    #[test]
    fn zero_elapsed_time_leaves_the_bands_unmoved() {
        let mut smoothing = SpectrumSmoothing::default();
        let bands = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::ZERO);
        assert_eq!(bands, [0.0; SPECTRUM_BANDS]);
    }

    #[test]
    fn fresh_is_settled() {
        let smoothing = SpectrumSmoothing::default();
        assert_eq!(smoothing.motion(), SpectrumMotion::Settled);
    }

    #[test]
    fn one_live_frame_moves() {
        let mut smoothing = SpectrumSmoothing::default();
        let _ = smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        assert_eq!(smoothing.motion(), SpectrumMotion::Moving);
    }

    #[test]
    fn fade_settles_within_seven_frames_at_33_ms() {
        let mut smoothing = SpectrumSmoothing::default();
        let _ = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::from_secs(1));
        let frame = Duration::from_millis(33);
        for _ in 0..7 {
            let _ = smoothing.fade(frame);
        }
        assert_eq!(smoothing.motion(), SpectrumMotion::Settled);
    }

    #[test]
    fn zero_elapsed_keeps_moving() {
        let mut smoothing = SpectrumSmoothing::default();
        let _ = smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        let _ = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::ZERO);
        assert_eq!(smoothing.motion(), SpectrumMotion::Moving);
    }

    #[test]
    fn a_tiny_band_snaps_to_zero() {
        let mut smoothing = SpectrumSmoothing::default();
        let mut raw = [0.0; SPECTRUM_BANDS];
        raw[0] = SILENT_BAND / 2.0;
        let bands = smoothing.smooth(&raw, FRAME);
        assert_eq!(bands[0], 0.0);
    }
}
