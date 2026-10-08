use std::{collections::VecDeque, iter, ops::Range};

use kernel::domain::{bounded::Bounded, speed::Speed};
use num_traits::ToPrimitive;
use rubato::{
    Adjustable,
    Async,
    FixedAsync,
    ResampleError,
    ResampleResult,
    Resampler,
    ResamplerConstructionError,
    SincInterpolationParameters,
    SincInterpolationType,
    WindowFunction,
    audioadapter_buffers::direct::InterleavedSlice,
};

pub(crate) const VARISPEED_FRAMES: usize = 64;
const HISTORY: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputFormat {
    pub(crate) channels: u16,
    pub(crate) rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conversion {
    Native,
    Resampled,
}

#[derive(Debug)]
pub(crate) struct Varispeed {
    conversion: Conversion,
    filter: Async<f32>,
    channels: usize,
    staged: VecDeque<f32>,
    carry: Vec<f32>,
    carried: Range<usize>,
    skip: usize,
    owed: Option<usize>,
    speed: Speed,
}

impl Varispeed {
    pub(crate) fn new(
        file_rate: u32,
        format: OutputFormat,
        speed: Speed,
    ) -> Result<Self, ResamplerConstructionError> {
        let channels = usize::from(format.channels.max(1));
        let parameters = SincInterpolationParameters {
            sinc_len: HISTORY,
            f_cutoff: None,
            oversampling_factor: 256,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris,
        };
        let filter = Async::new_sinc(
            f64::from(format.rate) / f64::from(file_rate),
            f64::from(Speed::MAX),
            &parameters,
            VARISPEED_FRAMES,
            channels,
            FixedAsync::Output,
        )?;
        let staged =
            VecDeque::with_capacity(filter.input_frames_max().max(HISTORY) * channels);
        let carry = vec![0.0; filter.output_frames_max() * channels];
        let mut varispeed = Self {
            conversion: Conversion::Native,
            filter,
            channels,
            staged,
            carry,
            carried: 0..0,
            skip: 0,
            owed: None,
            speed,
        };
        if file_rate != format.rate || speed != Speed::default() {
            varispeed.prime();
        }
        Ok(varispeed)
    }

    pub(crate) fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
        match self.conversion {
            Conversion::Native if speed == Speed::default() => {}
            Conversion::Native => self.prime(),
            Conversion::Resampled => {
                match self.filter.set_resample_ratio_relative(ratio(speed), true) {
                    Ok(()) | Err(_) => {}
                }
            }
        }
    }

    pub(crate) fn seek(&mut self) {
        self.staged.clear();
        self.carried = 0..0;
        self.owed = None;
        match self.conversion {
            Conversion::Native => {}
            Conversion::Resampled => self.prime(),
        }
    }

    pub(crate) fn fill(
        &mut self,
        input: &[f32],
        out: &mut [f32],
    ) -> ResampleResult<(usize, usize)> {
        match self.conversion {
            Conversion::Native => Ok(self.copy(input, out)),
            Conversion::Resampled => self.resample(input, out),
        }
    }

    #[must_use]
    pub(crate) fn input_frames_next(&self) -> usize {
        match self.conversion {
            Conversion::Native => VARISPEED_FRAMES,
            Conversion::Resampled => self
                .filter
                .input_frames_next()
                .saturating_sub(self.staged.len() / self.channels),
        }
    }

    pub(crate) fn flush(&mut self, out: &mut [f32]) -> ResampleResult<usize> {
        let channels = self.channels;
        let owed = match (self.conversion, self.owed) {
            (Conversion::Native, _) => 0,
            (Conversion::Resampled, Some(owed)) => owed,
            (Conversion::Resampled, None) => {
                self.tail().round().to_usize().unwrap_or(0)
            }
        };
        let wanted = owed.min(out.len() / channels);
        let mut written = 0;
        loop {
            let rest = out.split_at_mut(written * channels).1;
            written += self.deliver(rest.split_at_mut((wanted - written) * channels).0);
            if written == wanted {
                break;
            }
            let missing = self.input_frames_next();
            self.staged.extend(iter::repeat_n(0.0, missing * channels));
            self.convert()?;
        }
        self.owed = Some(owed - written);
        Ok(written)
    }

    fn tail(&self) -> f64 {
        let carried = float_count(self.carried.len() / self.channels);
        float_count(self.staged.len() / self.channels + HISTORY / 2).mul_add(
            self.filter.resample_ratio(),
            carried - float_count(self.skip),
        )
    }

    fn prime(&mut self) {
        self.filter.reset();
        match self
            .filter
            .set_resample_ratio_relative(ratio(self.speed), false)
        {
            Ok(()) | Err(_) => {}
        }
        let history = self.staged.len() / self.channels + HISTORY / 2;
        self.skip = (self.filter.resample_ratio() * (float_count(history) - 1.0))
            .floor()
            .to_usize()
            .unwrap_or(0);
        self.carried = 0..0;
        self.conversion = Conversion::Resampled;
    }

    fn copy(&mut self, input: &[f32], out: &mut [f32]) -> (usize, usize) {
        let frames = input.len().min(out.len()) / self.channels;
        let (copied, _) = input.split_at(frames * self.channels);
        out.split_at_mut(copied.len()).0.copy_from_slice(copied);
        self.remember(copied);
        (frames, frames)
    }

    fn remember(&mut self, copied: &[f32]) {
        let keep = HISTORY * self.channels;
        let (_, recent) = copied.split_at(copied.len().saturating_sub(keep));
        let overflow = (self.staged.len() + recent.len()).saturating_sub(keep);
        self.staged.drain(..overflow);
        self.staged.extend(recent);
    }

    fn resample(
        &mut self,
        input: &[f32],
        out: &mut [f32],
    ) -> ResampleResult<(usize, usize)> {
        let channels = self.channels;
        let wanted = out.len() / channels;
        self.owed = None;
        let mut read = 0;
        let mut written = 0;
        loop {
            written += self.deliver(out.split_at_mut(written * channels).1);
            if written == wanted {
                break;
            }
            read += self.stage(input.split_at(read * channels).1);
            if self.staged.len() / channels < self.filter.input_frames_next() {
                break;
            }
            self.convert()?;
        }
        Ok((read, written))
    }

    fn deliver(&mut self, out: &mut [f32]) -> usize {
        let channels = self.channels;
        let dropped = self.skip.min(self.carried.len() / channels);
        self.skip -= dropped;
        self.carried.start += dropped * channels;
        let frames = (self.carried.len() / channels).min(out.len() / channels);
        let samples = self
            .carry
            .iter()
            .skip(self.carried.start)
            .take(frames * channels);
        for (slot, sample) in out.iter_mut().zip(samples) {
            *slot = *sample;
        }
        self.carried.start += frames * channels;
        frames
    }

    fn stage(&mut self, input: &[f32]) -> usize {
        let missing = self.input_frames_next();
        let frames = missing.min(input.len() / self.channels);
        self.staged
            .extend(input.iter().take(frames * self.channels));
        frames
    }

    fn convert(&mut self) -> ResampleResult<()> {
        let channels = self.channels;
        let needed = self.filter.input_frames_next();
        let staged = self.staged.make_contiguous();
        let source =
            InterleavedSlice::new(&*staged, channels, needed).map_err(|_| {
                ResampleError::InsufficientInputBufferSize {
                    expected: needed,
                    actual: staged.len() / channels,
                }
            })?;
        let capacity = self.carry.len() / channels;
        let mut target = InterleavedSlice::new_mut(&mut self.carry, channels, capacity)
            .map_err(|_| ResampleError::InsufficientOutputBufferSize {
                expected: VARISPEED_FRAMES,
                actual: capacity,
            })?;
        let (consumed, produced) =
            self.filter
                .process_into_buffer(&source, &mut target, None)?;
        self.staged.drain(..consumed * channels);
        self.carried = 0..produced * channels;
        Ok(())
    }
}

fn ratio(speed: Speed) -> f64 {
    let limit = f64::from(Speed::MAX);
    (1.0 / f64::from(speed.get())).clamp(1.0 / limit, limit)
}

fn float_count(frames: usize) -> f64 {
    frames.to_f64().unwrap_or(f64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{f32::consts::TAU, time::Duration};

    use kernel::{
        cmd::Playback,
        domain::{bounded::Bounded, revision::Revision, speed::Speed},
    };
    use num_traits::ToPrimitive;
    use rstest::rstest;
    use rubato::Resampler;

    use crate::{
        deck::{
            feed::{FeedCmd, feed_channel, play},
            mixer::{MixerChannel, MixerOrder, mixer_channel},
            source::{
                DecodedTrack,
                decode,
                tests::{decoded, ramp_file},
            },
            varispeed::{Conversion, OutputFormat, Varispeed},
            voice::Voice,
        },
        engine::message::SinkRole,
        tap::spectrum_channel,
    };

    const STEREO_48K: OutputFormat = OutputFormat {
        channels: 2,
        rate: 48_000,
    };

    fn delay(varispeed: &Varispeed) -> usize {
        match varispeed.conversion {
            Conversion::Native => 0,
            Conversion::Resampled => (varispeed.tail()
                / varispeed.filter.resample_ratio())
            .round()
            .to_usize()
            .unwrap_or(0),
        }
    }

    fn sine(frequency: f32, rate: f32, frames: u16) -> Vec<f32> {
        (0..frames)
            .map(f32::from)
            .flat_map(|frame| {
                let value = 0.5 * (TAU * frequency * frame / rate).sin();
                [value, value]
            })
            .collect()
    }

    fn run(
        varispeed: &mut Varispeed,
        input: &[f32],
        frames: usize,
    ) -> (Vec<f32>, usize) {
        let channels = varispeed.channels;
        let mut out = vec![0.0; frames * channels];
        let (mut read, mut written) = (0, 0);
        while written < frames {
            let block = (written + 256).min(frames);
            let (taken, given) = varispeed
                .fill(
                    &input[read * channels..],
                    &mut out[written * channels..block * channels],
                )
                .unwrap();
            if taken == 0 && given == 0 {
                break;
            }
            read += taken;
            written += given;
        }
        assert_eq!(written, frames, "{read} frames read");
        (out, read)
    }

    fn left(samples: &[f32]) -> Vec<f32> {
        samples.iter().step_by(2).copied().collect()
    }

    fn largest_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn an_equal_rate_voice_at_speed_one_passes_frames_bit_exact() {
        let input: Vec<f32> = (0..1_024u16)
            .map(|index| f32::from(index) / 1_024.0 - 0.5)
            .collect();
        let mut varispeed =
            Varispeed::new(48_000, STEREO_48K, Speed::default()).unwrap();

        let (out, read) = run(&mut varispeed, &input, 512);

        assert_eq!(read, 512);
        assert_eq!(out, input);
        assert_eq!(delay(&varispeed), 0);
    }

    #[test]
    fn a_44100_sine_on_48000_keeps_its_pitch_and_level() {
        let input = sine(1_000.0, 44_100.0, 44_100);
        let mut varispeed =
            Varispeed::new(44_100, STEREO_48K, Speed::default()).unwrap();

        let (out, _) = run(&mut varispeed, &input, 30_000);

        let window = &left(&out)[1_000..25_000];
        let crossings = window
            .windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count();
        assert!(crossings.abs_diff(1_000) <= 1, "{crossings} zero crossings");
        let rms = (window.iter().map(|sample| sample * sample).sum::<f32>() / 24_000.0)
            .sqrt();
        let level = 20.0 * (rms / (0.5 / 2f32.sqrt())).log10();
        assert!(level.abs() < 0.1, "level off by {level} dB");
    }

    #[test]
    fn a_speed_step_continues_without_a_jump() {
        let input = sine(100.0, 48_000.0, 30_000);
        let mut varispeed =
            Varispeed::new(48_000, STEREO_48K, Speed::default()).unwrap();

        let (before, read) = run(&mut varispeed, &input, 4_800);
        varispeed.set_speed(Speed::clamped(1.25));
        let (after, _) = run(&mut varispeed, &input[read * 2..], 4_800);

        let samples = left(&[before, after].concat());
        let own = largest_step(&samples[..4_700]);
        let across = largest_step(&samples[4_700..]);
        assert!(across < 1.5 * own, "step {across} against {own}");
    }

    #[test]
    fn a_stereo_speed_step_plays_each_channel_as_a_mono_voice_does() {
        let stereo_input = sine(440.0, 48_000.0, 6_000);
        let mono_input = left(&stereo_input);
        let mono_format = OutputFormat {
            channels: 1,
            rate: 48_000,
        };
        let mut stereo = Varispeed::new(48_000, STEREO_48K, Speed::default()).unwrap();
        let mut mono = Varispeed::new(48_000, mono_format, Speed::default()).unwrap();

        let (_, stereo_read) = run(&mut stereo, &stereo_input, 300);
        let (_, mono_read) = run(&mut mono, &mono_input, 300);
        stereo.set_speed(Speed::clamped(1.3));
        mono.set_speed(Speed::clamped(1.3));
        let (stereo_after, _) =
            run(&mut stereo, &stereo_input[stereo_read * 2..], 2_000);
        let (mono_after, _) = run(&mut mono, &mono_input[mono_read..], 2_000);

        assert_eq!(left(&stereo_after), mono_after);
    }

    #[test]
    fn a_partly_staged_block_asks_only_for_its_missing_frames() {
        let input = sine(1_000.0, 44_100.0, 40);
        let mut varispeed =
            Varispeed::new(44_100, STEREO_48K, Speed::default()).unwrap();
        let needed = varispeed.input_frames_next();

        let filled = varispeed.fill(&input, &mut [0.0; 128]).unwrap();

        assert_eq!(filled, (40, 0));
        assert_eq!(varispeed.input_frames_next(), needed - 40);
    }

    #[rstest]
    #[case::before_the_filter_delay_passes(10, 64, 64)]
    #[case::after_a_partly_delivered_block(1_000, 650, 512)]
    #[case::into_short_buffers(1_000, 650, 4)]
    fn a_flush_renders_the_tail_silence_renders_and_ends_on_the_scaled_length(
        #[case] frames: u16,
        #[case] before: usize,
        #[case] piece: usize,
    ) {
        let input = sine(1_000.0, 44_100.0, frames);
        let mut flushed = Varispeed::new(44_100, STEREO_48K, Speed::default()).unwrap();
        let mut silenced =
            Varispeed::new(44_100, STEREO_48K, Speed::default()).unwrap();
        let (read, written) = flushed.fill(&input, &mut vec![0.0; before * 2]).unwrap();
        silenced.fill(&input, &mut vec![0.0; before * 2]).unwrap();

        let mut tail = Vec::new();
        loop {
            let mut out = vec![0.0_f32; piece * 2];
            let given = flushed.flush(&mut out).unwrap();
            tail.extend_from_slice(&out[..given * 2]);
            if given < piece {
                break;
            }
        }
        let mut silent = vec![0.0_f32; tail.len()];
        let (_, given) = silenced.fill(&[0.0; 4_096], &mut silent).unwrap();

        assert_eq!(given * 2, tail.len());
        assert_eq!(tail, silent);
        let played = written + tail.len() / 2;
        let scaled = read * 48_000 / 44_100;
        assert!(
            played.abs_diff(scaled) <= 3,
            "{read} read, {written} written, {} flushed",
            tail.len() / 2
        );
    }

    #[test]
    fn a_resampled_voice_sought_while_it_flushes_plays_the_target_with_no_frame_before_it()
     {
        const MONO_12K: OutputFormat = OutputFormat {
            channels: 1,
            rate: 12_000,
        };
        let file = ramp_file(1, 1_000);
        let expected = decoded(&file);
        let (feed_sender, feed_receiver) = crossbeam_channel::bounded(1);
        let (callback_sender, _callback_receiver) = crossbeam_channel::unbounded();
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder: decode(file.path()).unwrap(),
        };
        let (source, unserved) = feed_channel(decoded_track, 1, callback_sender);
        let (envelope, _control) = play(&source, unserved, &feed_sender);
        let Ok(FeedCmd::Serve(mut feed)) = feed_receiver.try_recv() else {
            panic!("play sends its feed to the feeder");
        };
        let varispeed = Varispeed::new(8_000, MONO_12K, Speed::default()).unwrap();
        let voice = Box::new(Voice::new(source, envelope, varispeed));
        let (spectrum_buffers, _spectrum_tap) = spectrum_channel();
        let MixerChannel {
            mut mixer,
            mut control,
            retired_voices: _retired_voices,
        } = mixer_channel(MONO_12K, Speed::default(), &spectrum_buffers);
        let role = SinkRole::Current;
        control.order(MixerOrder::Attach { role, voice });
        control.order(MixerOrder::Transport(Playback::Playing));
        mixer.mix(&mut vec![0.0_f32; 1_480]);

        control.order(MixerOrder::Seek(Duration::ZERO));
        mixer.mix::<f32>(&mut []);
        feed.prime();
        let mut sought = vec![1.0_f32; 600];
        mixer.mix(&mut sought);

        let mut fresh = Varispeed::new(8_000, MONO_12K, Speed::default()).unwrap();
        let mut target = vec![0.0_f32; 600];
        let (mut read, mut written) = (0, 0);
        while written < target.len() {
            let (taken, given) = fresh
                .fill(&expected[read..], &mut target[written..])
                .unwrap();
            assert!(taken + given > 0, "{read} read, {written} written");
            read += taken;
            written += given;
        }
        let gap = sought
            .iter()
            .zip(&target)
            .map(|(sample, wanted)| (sample - wanted).abs())
            .fold(0.0_f32, f32::max);
        assert!(gap < 1e-6, "{gap} off, starting {:?}", &sought[..4]);
    }

    #[rstest]
    #[case::two(Speed::clamped(2.0), 9_600, 3)]
    #[case::four(Speed::clamped(4.0), 19_200, 4)]
    #[case::a_quarter(Speed::clamped(0.25), 1_200, 4)]
    fn a_speed_consumes_its_multiple_of_the_frames(
        #[case] speed: Speed,
        #[case] expected: usize,
        #[case] tolerance: usize,
    ) {
        let input = sine(440.0, 48_000.0, 30_000);
        let mut varispeed = Varispeed::new(48_000, STEREO_48K, speed).unwrap();

        let (_, read) = run(&mut varispeed, &input, 4_800);

        let delayed = delay(&varispeed);
        let played = read - delayed;
        assert!(
            played.abs_diff(expected) <= tolerance,
            "speed {speed:?}: {read} read, {delayed} delayed"
        );
    }
}
