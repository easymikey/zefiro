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

pub(crate) struct Handoff {
    sender: Sender<Input<[f32; WINDOW]>>,
    receiver: Receiver<Input<[f32; WINDOW]>>,
}

impl fmt::Debug for Handoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handoff").finish_non_exhaustive()
    }
}

impl Handoff {
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
        new_tap().1
    }

    pub(crate) fn latest(&self, out: &mut [f32; WINDOW]) {
        *out = *self.output.borrow_mut().read();
    }
}

pub(crate) fn new_tap() -> (Handoff, SpectrumTap) {
    let (input, output) = triple_buffer(&[0.0_f32; WINDOW]);
    let (sender, receiver) = crossbeam_channel::bounded(1);
    match sender.try_send(input) {
        Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
    }
    (
        Handoff { sender, receiver },
        SpectrumTap {
            output: RefCell::new(output),
        },
    )
}

pub(crate) struct Tap<S> {
    inner: S,
    writer: Option<Writer>,
    give_back: Sender<Input<[f32; WINDOW]>>,
    frame: f32,
    channel: u16,
    hop: [f32; HOP],
    filled: usize,
}

impl<S> Tap<S>
where
    S: Source,
{
    pub(crate) fn new(inner: S, spectrum: &Handoff) -> Self {
        let writer = spectrum.take().map(|input| Writer {
            scratch: [0.0; WINDOW],
            input,
        });
        Self {
            inner,
            writer,
            give_back: spectrum.sender.clone(),
            frame: 0.0,
            channel: 0,
            hop: [0.0; HOP],
            filled: 0,
        }
    }
}

impl<S> Drop for Tap<S> {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            match self.give_back.try_send(writer.input) {
                Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                }
            }
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

    use crate::tap::{HOP, SpectrumTap, Tap, WINDOW, new_tap};

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
        let (spectrum, tap) = new_tap();
        let mut wrapped = Tap::new(
            Synthetic {
                samples: data.clone().into_iter(),
            },
            &spectrum,
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

    #[test]
    fn a_reopened_stream_gets_the_writer_back() {
        let (spectrum, tap) = new_tap();
        let first_batch = samples(HOP);
        let first = Tap::new(
            Synthetic {
                samples: first_batch.clone().into_iter(),
            },
            &spectrum,
        );
        first.for_each(drop);

        let mut out = [0.0f32; WINDOW];
        tap.latest(&mut out);
        assert_eq!(out.get(out.len() - HOP..), Some(first_batch.as_slice()));

        let second_batch = samples(HOP);
        let second = Tap::new(
            Synthetic {
                samples: second_batch.clone().into_iter(),
            },
            &spectrum,
        );
        second.for_each(drop);

        tap.latest(&mut out);
        assert_eq!(out.get(out.len() - HOP..), Some(second_batch.as_slice()));
    }

    #[test]
    fn a_second_tap_without_a_returned_writer_stays_silent() {
        let (spectrum, tap) = new_tap();
        let held = Tap::new(
            Synthetic {
                samples: Vec::new().into_iter(),
            },
            &spectrum,
        );
        let unlucky_batch = samples(HOP);
        let unlucky = Tap::new(
            Synthetic {
                samples: unlucky_batch.into_iter(),
            },
            &spectrum,
        );
        unlucky.for_each(drop);

        let mut out = [0.0f32; WINDOW];
        tap.latest(&mut out);
        assert!(out.iter().all(|&sample| sample == 0.0));
        drop(held);
    }

    #[test]
    fn a_handoff_lends_its_writer_only_once() {
        let (spectrum, _tap) = new_tap();
        assert!(spectrum.take().is_some());
        assert!(spectrum.take().is_none());
    }

    #[test]
    fn a_silent_tap_reads_an_empty_window() {
        let mut out = [1.0f32; WINDOW];
        SpectrumTap::silent().latest(&mut out);
        assert!(out.iter().all(|&sample| sample == 0.0));
    }
}
