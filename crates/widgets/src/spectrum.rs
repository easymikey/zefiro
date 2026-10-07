use std::time::Duration;

use kernel::{cmd::Playback, domain::time::Moment};

pub const SPECTRUM_BANDS: usize = 16;

pub type Spectrum = [f32; SPECTRUM_BANDS];

const REFERENCE_RATE_HZ: f32 = 60.0;
const SILENT_BAND: f32 = 1.0 / 1024.0;
const ATTACK: f32 = 0.85;
const DECAY: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpectrumMotion {
    Moving,
    Still,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpectrumFeed<'a> {
    Live(&'a Spectrum),
    Silent,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpectrumSmoothing {
    spectrum: Spectrum,
}

fn effective_coefficient(base: f32, elapsed_secs: f32) -> f32 {
    (base * elapsed_secs * REFERENCE_RATE_HZ).min(1.0)
}

fn smooth_band(previous: f32, raw: f32, coefficient: f32) -> f32 {
    previous + (raw - previous) * coefficient
}

impl SpectrumSmoothing {
    #[must_use]
    pub fn advance(&mut self, feed: SpectrumFeed<'_>, elapsed: Duration) -> Spectrum {
        match feed {
            SpectrumFeed::Live(raw) => self.smooth(raw, elapsed),
            SpectrumFeed::Silent => self.fade(elapsed),
        }
    }

    #[must_use]
    pub fn frame_due(
        &self,
        playback: Playback,
        next_frame_at: Moment,
    ) -> Option<Moment> {
        let is_due = match playback {
            Playback::Playing => true,
            Playback::Paused => self.motion() == SpectrumMotion::Moving,
        };
        is_due.then_some(next_frame_at)
    }

    #[must_use]
    fn smooth(&mut self, spectrum: &Spectrum, elapsed: Duration) -> Spectrum {
        let elapsed_secs = elapsed.as_secs_f32();
        let attack = effective_coefficient(ATTACK, elapsed_secs);
        let decay = effective_coefficient(DECAY, elapsed_secs);
        for (previous, &level) in self.spectrum.iter_mut().zip(spectrum.iter()) {
            let coefficient = if level > *previous { attack } else { decay };
            let smoothed = smooth_band(*previous, level, coefficient);
            *previous = if smoothed.abs() < SILENT_BAND {
                0.0
            } else {
                smoothed
            };
        }
        self.spectrum
    }

    #[must_use]
    fn fade(&mut self, elapsed: Duration) -> Spectrum {
        self.smooth(&[0.0; SPECTRUM_BANDS], elapsed)
    }

    #[must_use]
    pub fn bands(&self) -> &Spectrum {
        &self.spectrum
    }

    #[must_use]
    pub(crate) fn motion(&self) -> SpectrumMotion {
        if self.spectrum.iter().all(|&band| band == 0.0) {
            SpectrumMotion::Still
        } else {
            SpectrumMotion::Moving
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{cmd::Playback, domain::time::Moment};

    use crate::spectrum::{
        SILENT_BAND,
        SPECTRUM_BANDS,
        SpectrumFeed,
        SpectrumMotion,
        SpectrumSmoothing,
    };

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn smoothing_moves_every_band_toward_the_raw_value_without_overshoot() {
        let mut smoothing = SpectrumSmoothing::default();
        let target = [1.0; SPECTRUM_BANDS];
        let mut previous = *smoothing.bands();
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
        let bands = smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        assert_eq!(bands, *smoothing.bands());
        let mut decayed = *smoothing.bands();
        for _ in 0..30 {
            decayed = smoothing.fade(FRAME);
        }
        assert_eq!(decayed, [0.0; SPECTRUM_BANDS]);
    }

    #[test]
    fn bands_returns_the_same_values_without_advancing_them() {
        let mut smoothing = SpectrumSmoothing::default();
        let bands = smoothing.smooth(&[0.4; SPECTRUM_BANDS], FRAME);
        assert_eq!(bands, *smoothing.bands());
    }

    #[test]
    fn a_longer_elapsed_time_moves_further_toward_the_raw_value() {
        let mut short_smoothing = SpectrumSmoothing::default();
        let mut long_smoothing = SpectrumSmoothing::default();
        let target = [1.0; SPECTRUM_BANDS];
        let short_bands = short_smoothing.smooth(&target, Duration::from_millis(8));
        let long_bands = long_smoothing.smooth(&target, Duration::from_millis(32));
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
        assert_eq!(smoothing.motion(), SpectrumMotion::Still);
    }

    #[test]
    fn one_live_frame_moves() {
        let mut smoothing = SpectrumSmoothing::default();
        let bands = smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        assert_eq!(bands, *smoothing.bands());
        assert_eq!(smoothing.motion(), SpectrumMotion::Moving);
    }

    #[test]
    fn fade_settles_within_seven_frames_at_33_ms() {
        let mut smoothing = SpectrumSmoothing::default();
        let bands = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::from_secs(1));
        assert_eq!(bands, *smoothing.bands());
        let elapsed = Duration::from_millis(33);
        for _ in 0..7 {
            assert_eq!(smoothing.fade(elapsed), *smoothing.bands());
        }
        assert_eq!(smoothing.motion(), SpectrumMotion::Still);
    }

    #[test]
    fn zero_elapsed_keeps_moving() {
        let mut smoothing = SpectrumSmoothing::default();
        let first = smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        assert_eq!(first, *smoothing.bands());
        let second = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::ZERO);
        assert_eq!(second, *smoothing.bands());
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

    #[test]
    fn advance_follows_the_feed_and_a_silent_feed_fades() {
        let mut live_smoothing = SpectrumSmoothing::default();
        let raw = [1.0; SPECTRUM_BANDS];
        let lifted = live_smoothing.advance(SpectrumFeed::Live(&raw), FRAME);
        assert_eq!(lifted, *live_smoothing.bands());
        assert!(lifted.iter().all(|&band| band > 0.0));

        let faded = live_smoothing.advance(SpectrumFeed::Silent, FRAME);

        assert_eq!(faded, *live_smoothing.bands());
        assert!(
            faded
                .iter()
                .zip(lifted.iter())
                .all(|(after, before)| after < before)
        );
    }

    #[test]
    fn a_frame_is_due_only_while_bands_can_move() {
        let next_frame_at = Moment::new(Duration::from_millis(33));
        let settled_smoothing = SpectrumSmoothing::default();
        let mut moving_smoothing = SpectrumSmoothing::default();
        let lifted = moving_smoothing.smooth(&[1.0; SPECTRUM_BANDS], FRAME);
        assert_eq!(lifted, *moving_smoothing.bands());

        assert_eq!(
            settled_smoothing.frame_due(Playback::Playing, next_frame_at),
            Some(next_frame_at)
        );
        assert_eq!(
            moving_smoothing.frame_due(Playback::Paused, next_frame_at),
            Some(next_frame_at)
        );
        assert_eq!(
            settled_smoothing.frame_due(Playback::Paused, next_frame_at),
            None
        );
    }

    #[test]
    fn a_paused_spectrum_decays_to_settled_and_stops_asking_for_frames() {
        let next_frame_at = Moment::new(Duration::from_millis(33));
        let mut smoothing = SpectrumSmoothing::default();
        let lifted = smoothing.smooth(&[1.0; SPECTRUM_BANDS], Duration::from_secs(10));
        assert!(lifted.iter().all(|&band| band > 0.0));
        let mut frames = 0;
        while smoothing.motion() == SpectrumMotion::Moving && frames < 300 {
            let faded =
                smoothing.advance(SpectrumFeed::Silent, Duration::from_millis(33));
            assert!(faded.iter().all(|&band| band < 1.0));
            frames += 1;
        }

        assert!(frames < 300);
        assert_eq!(smoothing.frame_due(Playback::Paused, next_frame_at), None);
    }
}
