use std::{cell::RefCell, fmt, time::Duration};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use rodio::Source;
use triple_buffer::{Input, Output, triple_buffer};

pub(crate) const WINDOW: usize = 2048;
const HOP: usize = WINDOW / 4;

struct Writer {
    scratch: [f32; WINDOW],
    input: Input<[f32; WINDOW]>,
}

impl Writer {
    fn publish_hop(&mut self, hop: &[f32; HOP]) {
        self.scratch.rotate_left(HOP);
        if let Some(tail) = self.scratch.last_chunk_mut::<HOP>() {
            *tail = *hop;
        }
        self.input.write(self.scratch);
    }
}

pub(crate) struct SpectrumBuffers {
    sender: Sender<Input<[f32; WINDOW]>>,
    receiver: Receiver<Input<[f32; WINDOW]>>,
}

impl fmt::Debug for SpectrumBuffers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpectrumBuffers").finish_non_exhaustive()
    }
}

impl SpectrumBuffers {
    fn take(&self) -> Option<Input<[f32; WINDOW]>> {
        self.receiver.try_recv().ok()
    }
}

pub struct SpectrumTap {
    output: RefCell<Output<[f32; WINDOW]>>,
}

impl fmt::Debug for SpectrumTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpectrumTap").finish_non_exhaustive()
    }
}

impl SpectrumTap {
    #[must_use]
    pub fn silent() -> Self {
        spectrum_channel().1
    }

    pub(crate) fn latest(&self, out: &mut [f32; WINDOW]) {
        *out = *self.output.borrow_mut().read();
    }
}

pub(crate) fn spectrum_channel() -> (SpectrumBuffers, SpectrumTap) {
    let (input, output) = triple_buffer(&[0.0_f32; WINDOW]);
    let (sender, receiver) = crossbeam_channel::bounded(1);
    match sender.try_send(input) {
        Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
    }
    (
        SpectrumBuffers { sender, receiver },
        SpectrumTap {
            output: RefCell::new(output),
        },
    )
}

pub(crate) struct TappedSource<S> {
    inner: S,
    writer: Option<Writer>,
    return_sender: Sender<Input<[f32; WINDOW]>>,
    frame: f32,
    channel: u16,
    hop: [f32; HOP],
    filled: usize,
}

impl<S> TappedSource<S>
where
    S: Source,
{
    pub(crate) fn new(inner: S, spectrum_buffers: &SpectrumBuffers) -> Self {
        let writer = spectrum_buffers.take().map(|input| Writer {
            scratch: [0.0; WINDOW],
            input,
        });
        Self {
            inner,
            writer,
            return_sender: spectrum_buffers.sender.clone(),
            frame: 0.0,
            channel: 0,
            hop: [0.0; HOP],
            filled: 0,
        }
    }
}

impl<S> Drop for TappedSource<S> {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            match self.return_sender.try_send(writer.input) {
                Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                }
            }
        }
    }
}

impl<S> Iterator for TappedSource<S>
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
                if let Some(writer) = self.writer.as_mut() {
                    writer.publish_hop(&self.hop);
                }
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

impl<S> Source for TappedSource<S>
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

    use crate::tap::{HOP, SpectrumTap, TappedSource, WINDOW, spectrum_channel};

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
    fn a_tapped_source_batches_by_hop_and_publishes_the_newest_window(
        #[case] count: usize,
    ) {
        let data = samples(count);
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        let mut wrapped_source = TappedSource::new(
            Synthetic {
                samples: data.clone().into_iter(),
            },
            &spectrum_buffers,
        );

        let collected: Vec<f32> = wrapped_source.by_ref().collect();
        assert_eq!(collected, data);

        let mut out = [0.0f32; WINDOW];
        spectrum_tap.latest(&mut out);

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

    #[test]
    fn a_reopened_stream_gets_the_writer_back() {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        let first_batch = samples(HOP);
        let first = TappedSource::new(
            Synthetic {
                samples: first_batch.clone().into_iter(),
            },
            &spectrum_buffers,
        );
        first.for_each(drop);

        let mut out = [0.0f32; WINDOW];
        spectrum_tap.latest(&mut out);
        assert_eq!(out.get(out.len() - HOP..), Some(first_batch.as_slice()));

        let second_batch = samples(HOP);
        let second = TappedSource::new(
            Synthetic {
                samples: second_batch.clone().into_iter(),
            },
            &spectrum_buffers,
        );
        second.for_each(drop);

        spectrum_tap.latest(&mut out);
        assert_eq!(out.get(out.len() - HOP..), Some(second_batch.as_slice()));
    }

    #[test]
    fn a_second_tapped_source_without_a_returned_writer_stays_silent() {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        let held_source = TappedSource::new(
            Synthetic {
                samples: Vec::new().into_iter(),
            },
            &spectrum_buffers,
        );
        let unlucky_batch = samples(HOP);
        let unlucky_source = TappedSource::new(
            Synthetic {
                samples: unlucky_batch.into_iter(),
            },
            &spectrum_buffers,
        );
        unlucky_source.for_each(drop);

        let mut out = [0.0f32; WINDOW];
        spectrum_tap.latest(&mut out);
        assert!(out.iter().all(|&sample| sample == 0.0));
        drop(held_source);
    }

    #[test]
    fn spectrum_buffers_lend_their_writer_only_once() {
        let (spectrum_buffers, _spectrum_tap) = spectrum_channel();
        assert!(spectrum_buffers.take().is_some());
        assert!(spectrum_buffers.take().is_none());
    }

    #[test]
    fn a_silent_tap_reads_an_empty_window() {
        let mut out = [1.0f32; WINDOW];
        SpectrumTap::silent().latest(&mut out);
        assert!(out.iter().all(|&sample| sample == 0.0));
    }
}
