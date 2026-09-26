use std::sync::Arc;

use num_traits::ToPrimitive;
use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex};

use crate::tap::SpectrumTap;

pub struct SpectrumAnalyzer {
    transform: Arc<dyn RealToComplex<f32>>,
    window: Box<[f32; SpectrumAnalyzer::WINDOW]>,
    window_gain: f32,
    input: Box<[f32; SpectrumAnalyzer::WINDOW]>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl std::fmt::Debug for SpectrumAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpectrumAnalyzer").finish_non_exhaustive()
    }
}

impl SpectrumAnalyzer {
    pub const WINDOW: usize = 2048;

    #[must_use]
    pub fn new() -> Self {
        let transform = RealFftPlanner::<f32>::new().plan_fft_forward(Self::WINDOW);
        let spectrum = transform.make_output_vec();
        let scratch = transform.make_scratch_vec();
        let window = hann_window();
        let window_gain =
            window.iter().sum::<f32>() / usize_to_f32(Self::WINDOW.max(1));
        Self {
            transform,
            window: Box::new(window),
            window_gain,
            input: Box::new([0.0; Self::WINDOW]),
            spectrum,
            scratch,
        }
    }

    pub fn bands<const BANDS: usize>(&mut self, tap: &SpectrumTap) -> [f32; BANDS] {
        let mut samples = [0.0f32; Self::WINDOW];
        tap.latest(&mut samples);
        self.input
            .iter_mut()
            .zip(samples.iter().zip(self.window.iter()))
            .for_each(|(slot, (sample, window))| *slot = sample * window);
        let transformed = self.transform.process_with_scratch(
            &mut self.input[..],
            &mut self.spectrum,
            &mut self.scratch,
        );
        if transformed.is_err() {
            return [0.0; BANDS];
        }
        let usable = Self::WINDOW / 2;
        let scale = usize_to_f32(usable.max(1)) * self.window_gain;
        std::array::from_fn(|band| {
            let start = log_bin_edge(band, BANDS, usable);
            let end = log_bin_edge(band + 1, BANDS, usable)
                .max(start + 1)
                .min(usable);
            band_magnitude(self.spectrum.get(start..end).unwrap_or(&[]), scale)
        })
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn hann_window() -> [f32; SpectrumAnalyzer::WINDOW] {
    let denominator = usize_to_f32(SpectrumAnalyzer::WINDOW.max(2) - 1);
    std::array::from_fn(|index| {
        let phase = 2.0 * std::f32::consts::PI * usize_to_f32(index) / denominator;
        0.5 - 0.5 * phase.cos()
    })
}

fn log_bin_edge(band: usize, bands: usize, usable_bins: usize) -> usize {
    let fraction = usize_to_f32(band) / usize_to_f32(bands.max(1));
    let edge = usize_to_f32(usable_bins).powf(fraction).floor();
    floor_to_usize(edge).min(usable_bins)
}

fn band_magnitude(bins: &[Complex<f32>], scale: f32) -> f32 {
    let peak = bins.iter().map(|bin| bin.norm()).fold(0.0f32, f32::max);
    (peak / scale).sqrt().clamp(0.0, 1.0)
}

fn usize_to_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

fn floor_to_usize(value: f32) -> usize {
    value.to_usize().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rodio::Source;
    use rstest::rstest;

    use crate::{
        spectrum::SpectrumAnalyzer,
        tap::{Tap, new_tap},
    };

    struct Tone {
        samples: std::vec::IntoIter<f32>,
    }

    impl Iterator for Tone {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.samples.next()
        }
    }

    impl Source for Tone {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> u16 {
            1
        }
        fn sample_rate(&self) -> u32 {
            44_100
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    fn tone(cycles: f32) -> Vec<f32> {
        (0..SpectrumAnalyzer::WINDOW)
            .map(|index| {
                let phase = 2.0
                    * std::f32::consts::PI
                    * cycles
                    * crate::spectrum::usize_to_f32(index)
                    / crate::spectrum::usize_to_f32(SpectrumAnalyzer::WINDOW);
                phase.sin()
            })
            .collect()
    }

    #[test]
    fn silence_gives_zero_bands() {
        let (_spectrum, tap) = new_tap();
        let mut analyzer = SpectrumAnalyzer::new();
        let bands: [f32; 8] = analyzer.bands(&tap);
        assert!(bands.iter().all(|&band| band == 0.0));
    }

    #[rstest]
    #[case::a_few_bands(4)]
    #[case::many_bands(32)]
    fn every_band_stays_within_unit_range(#[case] count: usize) {
        let (spectrum, tap) = new_tap();
        let source = Tone {
            samples: tone(64.0).into_iter(),
        };
        Tap::new(source, &spectrum).for_each(drop);

        let mut analyzer = SpectrumAnalyzer::new();
        let bands: Vec<f32> = match count {
            4 => analyzer.bands::<4>(&tap).to_vec(),
            _ => analyzer.bands::<32>(&tap).to_vec(),
        };
        assert!(bands.iter().all(|&band| (0.0..=1.0).contains(&band)));
    }
}
