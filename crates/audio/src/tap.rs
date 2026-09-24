use std::{
    fmt,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use rodio::Source;
use triple_buffer::{Input, Output, triple_buffer};

use crate::spectrum::SpectrumAnalyzer;

const WINDOW: usize = SpectrumAnalyzer::WINDOW;
const HOP: usize = WINDOW / 4;

struct Writer {
    scratch: [f32; WINDOW],
    position: usize,
    input: Input<[f32; WINDOW]>,
}

impl Writer {
    fn publish_hop(&mut self, hop: &[f32; HOP]) {
        for &sample in hop {
            let slot = self.position % WINDOW;
            if let Some(cell) = self.scratch.get_mut(slot) {
                *cell = sample;
            }
            self.position = self.position.wrapping_add(1);
        }
        let oldest = self.position % WINDOW;
        let window: [f32; WINDOW] = std::array::from_fn(|offset| {
            self.scratch
                .get((oldest + offset) % WINDOW)
                .copied()
                .unwrap_or(0.0)
        });
        self.input.write(window);
    }
}

pub(crate) struct Ring {
    writer: Mutex<Writer>,
}

impl fmt::Debug for Ring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ring").finish_non_exhaustive()
    }
}

impl Ring {
    fn publish_hop(&self, hop: &[f32; HOP]) {
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        writer.publish_hop(hop);
    }
}

pub struct SpectrumTap {
    output: Mutex<Output<[f32; WINDOW]>>,
}

impl fmt::Debug for SpectrumTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpectrumTap").finish_non_exhaustive()
    }
}

impl SpectrumTap {
    #[must_use]
    pub fn silent() -> Self {
        new_tap().1
    }

    pub fn latest(&self, out: &mut [f32; WINDOW]) {
        if let Ok(mut output) = self.output.lock() {
            *out = *output.read();
        }
    }
}

pub(crate) fn new_tap() -> (Arc<Ring>, SpectrumTap) {
    let (input, output) = triple_buffer(&[0.0_f32; WINDOW]);
    let writer = Writer {
        scratch: [0.0; WINDOW],
        position: 0,
        input,
    };
    (
        Arc::new(Ring {
            writer: Mutex::new(writer),
        }),
        SpectrumTap {
            output: Mutex::new(output),
        },
    )
}

pub(crate) struct Tap<S> {
    inner: S,
    ring: Arc<Ring>,
    frame: f32,
    channel: u16,
    hop: [f32; HOP],
    filled: usize,
}

impl<S> Tap<S>
where
    S: Source,
{
    pub(crate) fn new(inner: S, ring: Arc<Ring>) -> Self {
        Self {
            inner,
            ring,
            frame: 0.0,
            channel: 0,
            hop: [0.0; HOP],
            filled: 0,
        }
    }
}

impl<S> Iterator for Tap<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        let channels = self.inner.channels();
        self.frame += sample;
        self.channel += 1;
        if self.channel >= channels {
            let averaged = self.frame / f32::from(channels);
            if let Some(cell) = self.hop.get_mut(self.filled) {
                *cell = averaged;
            }
            self.filled += 1;
            if self.filled >= HOP {
                self.ring.publish_hop(&self.hop);
                self.filled = 0;
            }
            self.frame = 0.0;
            self.channel = 0;
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for Tap<S>
where
    S: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rodio::Source;
    use rstest::rstest;

    use crate::tap::{HOP, Tap, WINDOW, new_tap};

    struct Synthetic {
        samples: std::vec::IntoIter<f32>,
    }

    impl Iterator for Synthetic {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.samples.next()
        }
    }

    impl Source for Synthetic {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> u16 {
            1
        }
        fn sample_rate(&self) -> u32 {
            44100
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    fn samples(count: usize) -> Vec<f32> {
        let count = u16::try_from(count).unwrap_or(u16::MAX);
        (0..count).map(f32::from).collect()
    }

    #[rstest]
    #[case::a_partial_hop_stays_unpublished(HOP - 1)]
    #[case::a_single_hop_publishes_its_tail(HOP)]
    #[case::wrapping_past_the_window(WINDOW + HOP)]
    fn the_tap_batches_by_hop_and_publishes_the_newest_window(#[case] count: usize) {
        let data = samples(count);
        let (ring, tap) = new_tap();
        let mut wrapped = Tap::new(
            Synthetic {
                samples: data.clone().into_iter(),
            },
            ring,
        );

        let collected: Vec<f32> = wrapped.by_ref().collect();
        assert_eq!(collected, data);

        let mut out = [0.0f32; WINDOW];
        tap.latest(&mut out);

        let flushed = count - (count % HOP);
        let kept = flushed.min(out.len());
        assert_eq!(
            out.get(out.len() - kept..),
            data.get(flushed - kept..flushed)
        );
        if kept < out.len() {
            let leading = out.get(..out.len() - kept).unwrap_or(&[]);
            assert!(leading.iter().all(|&sample| sample == 0.0));
        }
    }
}
