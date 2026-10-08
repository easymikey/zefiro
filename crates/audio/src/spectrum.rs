use std::sync::Arc;

use num_traits::ToPrimitive;
use realfft::{FftError, RealFftPlanner, RealToComplex, num_complex::Complex};

use crate::tap::{SpectrumTap, WINDOW};

pub struct SpectrumAnalyzer {
    transform: Arc<dyn RealToComplex<f32>>,
    window: Box<[f32; WINDOW]>,
    window_scale: f32,
    input: Box<[f32; WINDOW]>,
    bins: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl std::fmt::Debug for SpectrumAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpectrumAnalyzer").finish_non_exhaustive()
    }
}

impl SpectrumAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        let transform = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let bins = transform.make_output_vec();
        let scratch = transform.make_scratch_vec();
        let window = hann_window();
        let window_scale = window.iter().sum::<f32>() / float_count(WINDOW);
        Self {
            transform,
            window: Box::new(window),
            window_scale,
            input: Box::new([0.0; WINDOW]),
            bins,
            scratch,
        }
    }

    pub fn bands<const BANDS: usize>(
        &mut self,
        spectrum_tap: &SpectrumTap,
    ) -> [f32; BANDS] {
        if spectrum_tap.windowed(&self.window, &mut self.input) {
            match self.transform.process_with_scratch(
                &mut self.input[..],
                &mut self.bins,
                &mut self.scratch,
            ) {
                Ok(()) => {}
                Err(
                    FftError::InputBuffer(..)
                    | FftError::OutputBuffer(..)
                    | FftError::ScratchBuffer(..)
                    | FftError::InputValues(..),
                ) => self.bins.fill(Complex::default()),
            }
        }
        let usable = WINDOW / 2;
        let scale = float_count(usable) * self.window_scale;
        std::array::from_fn(|band| {
            let start = log_bin_edge(band, BANDS, usable);
            let end = log_bin_edge(band + 1, BANDS, usable)
                .max(start + 1)
                .min(usable);
            band_magnitude(self.bins.get(start..end).unwrap_or(&[]), scale)
        })
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn hann_window() -> [f32; WINDOW] {
    let denominator = float_count(WINDOW - 1);
    std::array::from_fn(|index| {
        let phase = 2.0 * std::f32::consts::PI * float_count(index) / denominator;
        0.5 - 0.5 * phase.cos()
    })
}

fn log_bin_edge(band: usize, bands: usize, usable_bins: usize) -> usize {
    let fraction = float_count(band) / float_count(bands.max(1));
    let edge = float_count(usable_bins).powf(fraction).floor();
    bin_index(edge).min(usable_bins)
}

fn band_magnitude(bins: &[Complex<f32>], scale: f32) -> f32 {
    let peak = bins.iter().map(|bin| bin.norm()).fold(0.0f32, f32::max);
    (peak / scale).sqrt().clamp(0.0, 1.0)
}

fn float_count(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

fn bin_index(fractional_bin: f32) -> usize {
    fractional_bin.to_usize().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use realfft::num_complex::Complex;
    use rstest::rstest;

    use crate::{
        spectrum::{SpectrumAnalyzer, band_magnitude, hann_window, log_bin_edge},
        tap::{SpectrumWriter, WINDOW, spectrum_channel},
    };

    fn tone(cycles: f32) -> Vec<f32> {
        (0..WINDOW)
            .map(|index| {
                let phase = 2.0
                    * std::f32::consts::PI
                    * cycles
                    * crate::spectrum::float_count(index)
                    / crate::spectrum::float_count(WINDOW);
                phase.sin()
            })
            .collect()
    }

    #[test]
    fn silence_gives_zero_bands() {
        let (_spectrum_buffers, spectrum_tap) = spectrum_channel();
        let mut analyzer = SpectrumAnalyzer::new();
        let bands: [f32; 8] = analyzer.bands(&spectrum_tap);
        assert!(bands.iter().all(|&band| band == 0.0));
    }

    #[test]
    fn a_failed_transform_gives_zero_bands_instead_of_the_last_spectrum() {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        SpectrumWriter::new(&spectrum_buffers, 1).push(&tone(64.0));
        let mut analyzer = SpectrumAnalyzer::new();
        let loud: [f32; 8] = analyzer.bands(&spectrum_tap);
        assert!(loud.iter().any(|&band| band > 0.0));

        analyzer.bins.truncate(WINDOW / 4);
        SpectrumWriter::new(&spectrum_buffers, 1).push(&tone(64.0));
        let failed: [f32; 8] = analyzer.bands(&spectrum_tap);
        assert!(failed.iter().all(|&band| band == 0.0), "{failed:?}");
    }

    #[rstest]
    #[case::a_few_bands(4)]
    #[case::many_bands(32)]
    fn every_band_stays_within_unit_range(#[case] count: usize) {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        SpectrumWriter::new(&spectrum_buffers, 1).push(&tone(64.0));

        let mut analyzer = SpectrumAnalyzer::new();
        let bands: Vec<f32> = match count {
            4 => analyzer.bands::<4>(&spectrum_tap).to_vec(),
            _ => analyzer.bands::<32>(&spectrum_tap).to_vec(),
        };
        assert!(bands.iter().all(|&band| (0.0..=1.0).contains(&band)));
    }

    #[rstest]
    #[case::a_low_tone_fills_every_band_that_holds_its_bin(1.0, 0..4)]
    #[case::a_middle_tone_fills_only_its_band(64.0, 19..20)]
    fn a_full_scale_tone_peaks_at_one_in_its_bands(
        #[case] cycles: f32,
        #[case] loud: Range<usize>,
    ) {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        SpectrumWriter::new(&spectrum_buffers, 1).push(&tone(cycles));

        let bands: [f32; 32] = SpectrumAnalyzer::new().bands(&spectrum_tap);
        assert!(
            bands.get(loud).is_some_and(|levels| levels
                .iter()
                .all(|level| (level - 1.0).abs() < 0.01)),
            "{bands:?}"
        );
    }

    #[test]
    fn the_hann_window_rises_from_zero_to_one_and_back_symmetrically() {
        let window = hann_window();
        let peak = window.iter().copied().fold(0.0f32, f32::max);
        assert!(
            window
                .first()
                .is_some_and(|&weight| weight.abs() < f32::EPSILON)
        );
        assert!(window.last().is_some_and(|&weight| weight.abs() < 1e-6));
        assert!(peak > 0.9999, "{peak}");
        assert!(window.iter().all(|weight| (0.0..=1.0).contains(weight)));
        assert!(
            window
                .iter()
                .zip(window.iter().rev())
                .all(|(rising, falling)| (rising - falling).abs() < 1e-4)
        );
    }

    #[rstest]
    #[case::the_first_band_starts_at_bin_one(0, 8, 1)]
    #[case::the_middle_band_starts_at_the_geometric_middle(4, 8, 32)]
    #[case::the_last_edge_is_the_usable_bin_count(8, 8, 1024)]
    #[case::no_bands_counts_as_one(0, 0, 1)]
    fn log_bin_edges_split_1024_bins_geometrically(
        #[case] band: usize,
        #[case] bands: usize,
        #[case] edge: usize,
    ) {
        assert_eq!(log_bin_edge(band, bands, 1024), edge);
    }

    #[rstest]
    #[case::no_bins_are_silent(vec![], 4.0, 0.0)]
    #[case::the_level_is_the_root_of_the_scaled_peak(vec![Complex::new(3.0, 4.0)], 25.0, 0.2f32.sqrt())]
    #[case::the_loudest_bin_wins(vec![Complex::new(1.0, 0.0), Complex::new(0.0, 2.0)], 16.0, 0.125f32.sqrt())]
    #[case::a_peak_above_the_scale_clamps_to_one(vec![Complex::new(6.0, 8.0)], 4.0, 1.0)]
    fn band_magnitude_scales_the_peak_bin(
        #[case] bins: Vec<Complex<f32>>,
        #[case] scale: f32,
        #[case] magnitude: f32,
    ) {
        assert!((band_magnitude(&bins, scale) - magnitude).abs() < 1e-6);
    }
}
