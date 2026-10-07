use std::{cell::RefCell, fmt};

use crossbeam_channel::{Receiver, Sender, TrySendError};
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

#[derive(Clone)]
pub struct SpectrumBuffers {
    sender: Sender<Input<[f32; WINDOW]>>,
    receiver: Receiver<Input<[f32; WINDOW]>>,
}

impl fmt::Debug for SpectrumBuffers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpectrumBuffers").finish_non_exhaustive()
    }
}

impl SpectrumBuffers {
    fn take(&self) -> Option<Writer> {
        self.receiver.try_recv().ok().map(|input| Writer {
            scratch: [0.0; WINDOW],
            input,
        })
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

    pub(crate) fn windowed(
        &self,
        window: &[f32; WINDOW],
        out: &mut [f32; WINDOW],
    ) -> bool {
        let mut output = self.output.borrow_mut();
        if !output.update() {
            return false;
        }
        out.iter_mut()
            .zip(output.output_buffer().iter().zip(window))
            .for_each(|(slot, (sample, weight))| *slot = sample * weight);
        true
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

pub(crate) struct SpectrumWriter {
    spectrum_buffers: SpectrumBuffers,
    writer: Option<Writer>,
    channels: u16,
    hop: [f32; HOP],
    filled: usize,
}

impl fmt::Debug for SpectrumWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpectrumWriter")
            .field("channels", &self.channels)
            .field("filled", &self.filled)
            .finish_non_exhaustive()
    }
}

impl SpectrumWriter {
    pub(crate) fn new(spectrum_buffers: &SpectrumBuffers, channels: u16) -> Self {
        Self {
            spectrum_buffers: spectrum_buffers.clone(),
            writer: spectrum_buffers.take(),
            channels: channels.max(1),
            hop: [0.0; HOP],
            filled: 0,
        }
    }

    pub(crate) fn push(&mut self, frames: &[f32]) {
        let width = f32::from(self.channels);
        for frame in frames.chunks_exact(usize::from(self.channels)) {
            if let Some(cell) = self.hop.get_mut(self.filled) {
                *cell = frame.iter().sum::<f32>() / width;
            }
            self.filled += 1;
            if self.filled >= HOP {
                if self.writer.is_none() {
                    self.writer = self.spectrum_buffers.take();
                }
                if let Some(writer) = self.writer.as_mut() {
                    writer.publish_hop(&self.hop);
                }
                self.filled = 0;
            }
        }
    }
}

impl Drop for SpectrumWriter {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            match self.spectrum_buffers.sender.try_send(writer.input) {
                Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::tap::{HOP, SpectrumTap, SpectrumWriter, WINDOW, spectrum_channel};

    fn samples(count: usize) -> Vec<f32> {
        let count = u16::try_from(count).unwrap_or(u16::MAX);
        (0..count).map(f32::from).collect()
    }

    #[rstest]
    #[case::a_partial_hop_stays_unpublished(HOP - 1)]
    #[case::a_single_hop_publishes_its_tail(HOP)]
    #[case::wrapping_past_the_window(WINDOW + HOP)]
    fn a_spectrum_writer_batches_by_hop_and_publishes_the_newest_window(
        #[case] count: usize,
    ) {
        let data = samples(count);
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        SpectrumWriter::new(&spectrum_buffers, 1).push(&data);

        let mut out = [0.0f32; WINDOW];
        let fresh = spectrum_tap.windowed(&[1.0; WINDOW], &mut out);

        let flushed = count - (count % HOP);
        assert_eq!(fresh, flushed > 0);
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
        SpectrumWriter::new(&spectrum_buffers, 1).push(&first_batch);

        let mut out = [0.0f32; WINDOW];
        assert!(spectrum_tap.windowed(&[1.0; WINDOW], &mut out));
        assert_eq!(out.get(out.len() - HOP..), Some(first_batch.as_slice()));

        let second_batch = samples(HOP);
        SpectrumWriter::new(&spectrum_buffers, 1).push(&second_batch);

        assert!(spectrum_tap.windowed(&[1.0; WINDOW], &mut out));
        assert_eq!(out.get(out.len() - HOP..), Some(second_batch.as_slice()));
    }

    #[test]
    fn a_second_spectrum_writer_without_a_returned_writer_stays_silent() {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        let held_spectrum_writer = SpectrumWriter::new(&spectrum_buffers, 1);
        SpectrumWriter::new(&spectrum_buffers, 1).push(&samples(HOP));

        let mut out = [0.0f32; WINDOW];
        assert!(!spectrum_tap.windowed(&[1.0; WINDOW], &mut out));
        assert!(out.iter().all(|&sample| sample == 0.0));
        drop(held_spectrum_writer);
    }

    #[test]
    fn a_spectrum_writer_built_while_another_holds_the_window_takes_it_once_the_other_drops()
     {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        let older_spectrum_writer = SpectrumWriter::new(&spectrum_buffers, 1);
        let mut newer_spectrum_writer = SpectrumWriter::new(&spectrum_buffers, 1);
        let newer_batch = samples(HOP);
        drop(older_spectrum_writer);
        newer_spectrum_writer.push(&newer_batch);

        let mut out = [0.0f32; WINDOW];
        assert!(spectrum_tap.windowed(&[1.0; WINDOW], &mut out));
        assert_eq!(out.get(out.len() - HOP..), Some(newer_batch.as_slice()));
    }

    #[test]
    fn a_hop_of_frames_publishes_one_window_of_their_channel_average() {
        let (spectrum_buffers, spectrum_tap) = spectrum_channel();
        let mut spectrum_writer = SpectrumWriter::new(&spectrum_buffers, 2);
        let stereo: Vec<f32> = samples(HOP)
            .iter()
            .flat_map(|&sample| [sample, sample + 2.0])
            .collect();
        let (head, tail) = stereo.split_at(HOP);
        let mut out = [0.0f32; WINDOW];

        spectrum_writer.push(head);
        assert!(!spectrum_tap.windowed(&[1.0; WINDOW], &mut out));
        spectrum_writer.push(tail);
        assert!(spectrum_tap.windowed(&[1.0; WINDOW], &mut out));
        assert!(!spectrum_tap.windowed(&[1.0; WINDOW], &mut out));

        let averaged: Vec<f32> =
            samples(HOP).iter().map(|sample| sample + 1.0).collect();
        assert_eq!(out.get(WINDOW - HOP..), Some(averaged.as_slice()));
    }

    #[test]
    fn spectrum_buffers_lend_their_writer_only_once() {
        let (spectrum_buffers, _spectrum_tap) = spectrum_channel();
        assert!(spectrum_buffers.take().is_some());
        assert!(spectrum_buffers.take().is_none());
    }

    #[test]
    fn a_silent_tap_reads_an_empty_window() {
        let mut out = [0.0f32; WINDOW];
        assert!(!SpectrumTap::silent().windowed(&[1.0; WINDOW], &mut out));
        assert!(out.iter().all(|&sample| sample == 0.0));
    }
}
