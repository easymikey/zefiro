use std::{sync::atomic::Ordering, time::Duration};

use kernel::cmd::Playback;
use rtrb::{PopError, PushError};

use crate::deck::{
    envelope::Frames,
    feed::{Chunk, ChunkMark, FeedSource, UNDERRUN_FRAMES},
};

impl Chunk {
    fn position(&self, samples: usize) -> Duration {
        let frames = samples / usize::from(self.channels.max(1));
        let frames = u64::try_from(frames).unwrap_or(u64::MAX);
        Frames(self.first_frame.saturating_add(frames)).duration(self.rate)
    }
}

impl FeedSource {
    fn arrived(&mut self) -> bool {
        let generation = self.generation;
        let slots = self.full_consumer.slots();
        self.full_consumer.read_chunk(slots).is_ok_and(|queued| {
            let (head, tail) = queued.as_slices();
            tail.last()
                .or(head.last())
                .is_some_and(|chunk| chunk.generation == generation)
        })
    }

    fn refused(&mut self) {
        if self.seek_target.refused.load(Ordering::Acquire) == self.generation {
            self.generation = self.seek_target.kept.load(Ordering::Relaxed);
        }
    }

    fn stale(&mut self) -> bool {
        let generation = self.generation;
        if self
            .held_chunk
            .as_ref()
            .is_none_or(|chunk| chunk.generation != generation)
        {
            self.refused();
        }
        let Some(chunk) = &self.held_chunk else {
            return true;
        };
        if chunk.generation == self.generation
            || !self
                .played
                .is_multiple_of(usize::from(chunk.channels.max(1)))
        {
            return false;
        }
        self.played >= chunk.len || self.playback == Playback::Paused || self.arrived()
    }

    fn advance(&mut self) {
        if let Some(chunk) = self.held_chunk.take() {
            self.release(chunk);
        }
        self.refused();
        self.played = 0;
        let arrived = match self.playback {
            Playback::Playing => self.arrived(),
            Playback::Paused => true,
        };
        loop {
            match self.full_consumer.pop() {
                Ok(chunk)
                    if (chunk.generation == self.generation || !arrived)
                        && (chunk.len > 0 || chunk.mark == ChunkMark::End) =>
                {
                    self.channels = chunk.channels;
                    self.rate = chunk.rate;
                    if chunk.generation == self.generation {
                        self.envelope_readout.publish(chunk.position(0));
                    }
                    self.held_chunk = Some(chunk);
                    return;
                }
                Ok(chunk) => self.release(chunk),
                Err(PopError::Empty) if self.full_consumer.is_abandoned() => {
                    if self.full_consumer.is_empty() {
                        return;
                    }
                }
                Err(PopError::Empty) => {
                    self.silence_left =
                        UNDERRUN_FRAMES * usize::from(self.channels.max(1));
                    return;
                }
            }
        }
    }

    #[sanitize(realtime = "nonblocking")]
    pub(crate) fn read(&mut self, out: &mut [f32]) -> usize {
        let mut written = 0;
        while let Some(rest) = out.get_mut(written..)
            && !rest.is_empty()
        {
            if self.silence_left == 0 && self.stale() {
                self.advance();
            }
            if self.silence_left > 0 {
                let silent = self.silence_left.min(rest.len());
                if let Some(target) = rest.get_mut(..silent) {
                    target.fill(0.0);
                }
                self.silence_left -= silent;
                written += silent;
                continue;
            }
            let Some(chunk) = self.held_chunk.as_ref() else {
                break;
            };
            let held = chunk.samples.get(self.played..chunk.len).unwrap_or(&[]);
            let copied = held.len().min(rest.len());
            let (Some(target), Some(samples)) =
                (rest.get_mut(..copied), held.get(..copied))
            else {
                break;
            };
            if copied == 0 {
                break;
            }
            target.copy_from_slice(samples);
            self.played += copied;
            written += copied;
            if chunk.generation == self.generation {
                self.envelope_readout.publish(chunk.position(self.played));
            }
            if self.played == chunk.len && chunk.mark == ChunkMark::Samples {
                self.advance();
            }
        }
        written
    }

    pub(crate) fn channels(&self) -> u16 {
        self.channels
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub(crate) fn seek(&mut self, position: Duration) {
        self.playback = match self.held_chunk {
            Some(_) => self.envelope_readout.playback(),
            None => Playback::Paused,
        };
        self.generation = self
            .seek_target
            .generation
            .load(Ordering::Relaxed)
            .wrapping_add(1);
        let nanos = u64::try_from(position.as_nanos()).unwrap_or(u64::MAX);
        self.seek_target
            .position_nanos
            .store(nanos, Ordering::Relaxed);
        self.seek_target
            .generation
            .store(self.generation, Ordering::Release);
        self.envelope_readout.publish(position);
    }

    fn release(&mut self, chunk: Chunk) {
        match self.empty_producer.push(chunk) {
            Ok(()) | Err(PushError::Full(_)) => {}
        }
    }
}

impl Drop for FeedSource {
    fn drop(&mut self) {
        if let Some(chunk) = self.held_chunk.take() {
            self.release(chunk);
        }
    }
}
