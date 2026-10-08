mod channels;
mod growing;
pub(crate) mod serve;
mod source;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use channels::map_channels;
use crossbeam_channel::{SendError, Sender, TrySendError};
use kernel::{cmd::Playback, domain::revision::Revision, message::AudioError};
use rtrb::{Consumer, PopError, Producer, PushError, RingBuffer};

use crate::{
    deck::{
        envelope::{Envelope, EnvelopeControl, EnvelopeReadout, Frames, envelope},
        event::DeckEvent,
        source::{DecodedTrack, TrackDecoder},
        varispeed::OutputFormat,
    },
    engine::message::{AudioMessage, EngineMessage, Signals},
    error::{decode_error, seek_error},
};

const FEED_SECONDS: Duration = Duration::from_millis(1_500);
const CHUNK_COUNT: usize = 8;
const SEEK_SPARES: usize = 2;
const PRIME_CHUNKS: usize = 2;
const UNDERRUN_FRAMES: usize = 256;

#[derive(Debug)]
pub(crate) struct Chunk {
    samples: Box<[f32]>,
    len: usize,
    channels: u16,
    rate: u32,
    generation: u32,
    first_frame: u64,
    mark: ChunkMark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkMark {
    Samples,
    End,
}

#[derive(Debug, Default)]
struct SeekTarget {
    generation: AtomicU32,
    position_nanos: AtomicU64,
    refused: AtomicU32,
    kept: AtomicU32,
}

#[derive(Debug)]
pub(crate) struct FeedSource {
    full_consumer: Consumer<Chunk>,
    empty_producer: Producer<Chunk>,
    held_chunk: Option<Chunk>,
    played: usize,
    silence_left: usize,
    channels: u16,
    rate: u32,
    generation: u32,
    playback: Playback,
    seek_target: Arc<SeekTarget>,
    envelope_readout: Arc<EnvelopeReadout>,
}

#[derive(Debug)]
pub struct Feed {
    decoder: TrackDecoder,
    empty_consumer: Consumer<Chunk>,
    full_producer: Producer<Chunk>,
    seek_target: Arc<SeekTarget>,
    generation: u32,
    sought: u32,
    frames: Frames,
    phase: FeedPhase,
    spare_chunks: Vec<Chunk>,
    callback_sender: Sender<AudioMessage>,
    envelope_readout: Arc<EnvelopeReadout>,
    revision: Revision,
    signals: Signals,
    wake: Wake,
    buffering_wake: Wake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedPhase {
    Decoding,
    Buffering,
    Drained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    Sent,
    Pending,
}

pub enum FeedCmd {
    Serve(Box<Feed>),
    Pace(Playback),
}

impl std::fmt::Debug for FeedCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serve(_feed) => f.write_str("Serve"),
            Self::Pace(playback) => f.debug_tuple("Pace").field(playback).finish(),
        }
    }
}

#[must_use]
pub(crate) fn feed_channel(
    decoded_track: DecodedTrack,
    channels: u16,
    callback_sender: Sender<AudioMessage>,
) -> (FeedSource, Feed) {
    let DecodedTrack { revision, decoder } = decoded_track;
    let (mut empty_producer, empty_consumer) =
        RingBuffer::new(CHUNK_COUNT + SEEK_SPARES);
    let (full_producer, full_consumer) = RingBuffer::new(CHUNK_COUNT + SEEK_SPARES);
    let rate = decoder.sample_rate();
    let frames = usize::try_from((FEED_SECONDS * rate).as_secs()).unwrap_or(usize::MAX)
        / CHUNK_COUNT;
    let samples = frames.max(1).saturating_mul(usize::from(channels.max(1)));
    let mut spare_chunks: Vec<Chunk> = (0..CHUNK_COUNT + SEEK_SPARES)
        .map(|_| Chunk {
            samples: vec![0.0; samples].into_boxed_slice(),
            len: 0,
            channels,
            rate,
            generation: 0,
            first_frame: 0,
            mark: ChunkMark::Samples,
        })
        .collect();
    for chunk in spare_chunks.drain(SEEK_SPARES..) {
        match empty_producer.push(chunk) {
            Ok(()) | Err(PushError::Full(_)) => {}
        }
    }
    let seek_target = Arc::<SeekTarget>::default();
    let envelope_readout = Arc::<EnvelopeReadout>::default();
    let source = FeedSource {
        full_consumer,
        empty_producer,
        held_chunk: None,
        played: 0,
        silence_left: 0,
        channels,
        rate,
        generation: 0,
        playback: Playback::Playing,
        seek_target: Arc::clone(&seek_target),
        envelope_readout: Arc::clone(&envelope_readout),
    };
    let feed = Feed {
        decoder,
        empty_consumer,
        full_producer,
        seek_target,
        generation: 0,
        sought: 0,
        frames: Frames::ZERO,
        phase: FeedPhase::Decoding,
        spare_chunks,
        callback_sender,
        envelope_readout,
        revision,
        signals: Signals::default(),
        wake: Wake::Sent,
        buffering_wake: Wake::Sent,
    };
    (source, feed)
}

impl Feed {
    pub fn prime(&mut self) {
        for _ in 0..PRIME_CHUNKS {
            match self.empty_consumer.pop() {
                Ok(chunk) => self.refill(chunk),
                Err(PopError::Empty) => return,
            }
        }
    }

    pub(crate) fn serve(&mut self) {
        if let Some(spare) = self.spare_chunks.pop() {
            self.refill(spare);
        }
        while let Ok(chunk) = self.empty_consumer.pop() {
            self.refill(chunk);
        }
    }

    fn refill(&mut self, chunk: Chunk) {
        let due = self.due_seek();
        if due.is_none()
            && (self.phase != FeedPhase::Decoding
                || self.spare_chunks.len() < SEEK_SPARES)
        {
            self.spare_chunks.push(chunk);
            return;
        }
        if due.is_none() && !self.has_margin() {
            self.phase = FeedPhase::Buffering;
            self.buffering_wake = Wake::Pending;
            self.spare_chunks.push(chunk);
            return;
        }
        let mut next = Some(chunk);
        while let Some(mut current) = next {
            self.fill(
                &mut current,
                due.filter(|generation| *generation != self.sought),
            );
            match self.full_producer.push(current) {
                Ok(()) | Err(PushError::Full(_)) => {}
            }
            next = match self.phase {
                FeedPhase::Decoding if due.is_some() && self.has_margin() => {
                    self.spare_chunks.pop()
                }
                FeedPhase::Decoding | FeedPhase::Buffering | FeedPhase::Drained => None,
            };
        }
    }

    fn fill(&mut self, chunk: &mut Chunk, due: Option<u32>) {
        chunk.mark = self.seek(due);
        chunk.generation = self.generation;
        chunk.first_frame = self.frames.0;
        chunk.rate = self.decoder.sample_rate();
        chunk.len = 0;
        if chunk.mark == ChunkMark::End {
            self.phase = FeedPhase::Drained;
            return;
        }
        let channels = usize::from(chunk.channels.max(1));
        loop {
            let available = match self.decoder.frames() {
                Ok(samples) => samples.len(),
                Err(source) => {
                    let path = self.decoder.path.to_path_buf();
                    let error = decode_error(&source);
                    let error = AudioError::Decode { path, error };
                    self.report(EngineMessage::Interrupted(self.revision, error));
                    0
                }
            };
            if available > 0 && self.decoder.sample_rate() != chunk.rate {
                if chunk.len > 0 {
                    return;
                }
                chunk.rate = self.decoder.sample_rate();
            }
            let positions = self.decoder.spec.channels;
            let file_channels = usize::from(self.decoder.channels().max(1));
            let frames = (available / file_channels)
                .min(chunk.samples.len().saturating_sub(chunk.len) / channels);
            if available < file_channels {
                chunk.mark = ChunkMark::End;
                self.phase = FeedPhase::Drained;
                return;
            }
            if frames == 0 {
                return;
            }
            let count = frames * channels;
            let Ok(samples) = self.decoder.frames() else {
                return;
            };
            let slots = chunk
                .samples
                .get_mut(chunk.len..chunk.len + count)
                .unwrap_or(&mut []);
            for (from, to) in samples
                .chunks_exact(file_channels)
                .zip(slots.chunks_exact_mut(channels))
            {
                map_channels(positions, from, to);
            }
            chunk.len += count;
            self.decoder.consume(frames);
            self.frames.0 = self
                .frames
                .0
                .saturating_add(u64::try_from(frames).unwrap_or(u64::MAX));
        }
    }

    fn seek(&mut self, due: Option<u32>) -> ChunkMark {
        let Some(generation) = due else {
            return ChunkMark::Samples;
        };
        self.sought = generation;
        let rate = self.decoder.sample_rate();
        let position = Duration::from_nanos(
            self.seek_target.position_nanos.load(Ordering::Relaxed),
        );
        let landed = match self.decoder.seek(position) {
            Ok(landed) => {
                self.generation = generation;
                landed
            }
            Err(error) => {
                self.seek_target
                    .kept
                    .store(self.generation, Ordering::Relaxed);
                self.seek_target
                    .refused
                    .store(generation, Ordering::Release);
                self.report(EngineMessage::Error(seek_error(&error)));
                let sought = self.decoder.seek(self.frames.duration(rate));
                let Ok(landed) = sought.inspect_err(|failed| {
                    self.report(EngineMessage::Error(seek_error(failed)));
                }) else {
                    return ChunkMark::End;
                };
                landed
            }
        };
        if self.phase == FeedPhase::Drained {
            self.phase = FeedPhase::Decoding;
        }
        self.frames = Frames::from_duration(landed, rate);
        ChunkMark::Samples
    }

    fn report(&self, message: EngineMessage) {
        match self.callback_sender.try_send(AudioMessage::Engine(message)) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }

    fn wake(&mut self) {
        let raised = Signals(self.envelope_readout.flags.load(Ordering::Acquire));
        if self.wake == Wake::Pending || raised.0 & !self.signals.0 != 0 {
            let woke = DeckEvent::Woke(self.revision).wake(&self.callback_sender);
            self.wake = match woke {
                Ok(()) => Wake::Sent,
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                    Wake::Pending
                }
            };
        }
        self.signals = raised;
    }
}

pub(crate) fn play(
    source: &FeedSource,
    mut feed: Feed,
    feed_sender: &Sender<FeedCmd>,
) -> (Envelope, EnvelopeControl) {
    let revision = feed.revision;
    let envelope_readout = Arc::clone(&feed.envelope_readout);
    feed.prime();
    match feed_sender.send(FeedCmd::Serve(Box::new(feed))) {
        Ok(()) | Err(SendError(_)) => {}
    }
    let format = OutputFormat {
        channels: source.channels(),
        rate: source.sample_rate(),
    };
    envelope(format, revision, envelope_readout)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{io::Write, time::Duration};

    use crossbeam_channel::{Sender, TryRecvError};
    use kernel::{
        domain::revision::Revision,
        message::{AudioError, DecodeError},
    };
    use tempfile::NamedTempFile;

    use crate::{
        deck::{
            feed::{
                CHUNK_COUNT,
                ChunkMark,
                FEED_SECONDS,
                Feed,
                FeedSource,
                SEEK_SPARES,
                UNDERRUN_FRAMES,
                feed_channel,
            },
            source::{
                DecodedTrack,
                decode,
                tests::{decoded, ramp_file},
            },
        },
        engine::message::{AudioMessage, EngineMessage},
    };

    pub(crate) fn fed(
        file: &NamedTempFile,
        callback_sender: Sender<AudioMessage>,
    ) -> (FeedSource, Feed) {
        let decoder = decode(file.path()).unwrap();
        let channels = decoder.channels();
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder,
        };
        feed_channel(decoded_track, channels, callback_sender)
    }

    pub(crate) fn opened(file: &NamedTempFile) -> (FeedSource, Feed) {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        fed(file, callback_sender)
    }

    pub(crate) fn pulled(source: &mut FeedSource, samples: usize) -> Vec<f32> {
        let mut out = vec![0.0; samples];
        let read = source.read(&mut out);
        out.truncate(read);
        out
    }

    #[test]
    fn a_dropped_feed_ends_the_source_after_the_chunks_already_sent() {
        let file = ramp_file(1, 40_000);
        let expected = decoded(&file);
        let (mut source, mut feed) = opened(&file);
        let chunk_len = feed.empty_consumer.peek().unwrap().samples.len();
        feed.prime();
        drop(feed);
        assert_eq!(
            pulled(&mut source, expected.len()),
            expected[..2 * chunk_len]
        );
        assert_eq!(pulled(&mut source, 1), []);
    }

    #[test]
    fn an_underrun_plays_whole_silent_frames_then_the_next_chunk() {
        let file = ramp_file(2, 1_000);
        let expected = decoded(&file);
        let (mut source, mut feed) = opened(&file);
        let silence = UNDERRUN_FRAMES * 2;
        assert_eq!(pulled(&mut source, 1), [0.0]);
        feed.prime();
        assert_eq!(pulled(&mut source, silence - 1), vec![0.0; silence - 1]);
        assert_eq!(pulled(&mut source, 1).first(), expected.first());
    }

    #[test]
    fn a_fill_after_a_seek_starts_at_the_target() {
        let file = ramp_file(1, 40_000);
        let expected = decoded(&file);
        let (mut source, mut feed) = opened(&file);
        source.seek(Duration::from_secs(1));
        let mut chunk = feed.empty_consumer.pop().unwrap();
        feed.fill(&mut chunk, feed.due_seek());
        assert_eq!(chunk.generation, 1);
        assert_eq!(chunk.samples.first(), expected.get(8_000));
    }

    #[test]
    fn a_seek_after_the_end_refills_the_parked_chunks_from_the_target() {
        let chunk_len =
            usize::try_from((FEED_SECONDS * 8_000).as_secs()).unwrap() / CHUNK_COUNT;
        let file = ramp_file(1, 5 * chunk_len / 2);
        let expected = decoded(&file);
        let (mut source, mut feed) = opened(&file);
        feed.prime();
        let mut played = pulled(&mut source, 2 * chunk_len);
        feed.prime();
        played.extend(
            pulled(&mut source, 4 * expected.len())
                .into_iter()
                .filter(|sample| *sample != 0.0),
        );
        assert_eq!((played == expected, pulled(&mut source, 1)), (true, vec![]));
        assert_eq!(feed.spare_chunks.len(), SEEK_SPARES + 1);
        source.seek(Duration::ZERO);
        feed.prime();
        assert_eq!(feed.spare_chunks.len(), SEEK_SPARES);
        let replayed: Vec<f32> = pulled(&mut source, 4 * expected.len())
            .into_iter()
            .filter(|sample| *sample != 0.0)
            .collect();
        assert_eq!(replayed.len(), expected.len());
        assert!(replayed == expected);
    }

    #[test]
    fn a_fill_across_packets_keeps_every_frame_in_order() {
        let file = ramp_file(2, 20_000);
        let expected = decoded(&file);
        let (_source, mut feed) = opened(&file);
        let mut chunk = feed.empty_consumer.pop().unwrap();
        let mut filled = Vec::new();
        let mut lengths = Vec::new();
        while chunk.mark == ChunkMark::Samples {
            feed.fill(&mut chunk, feed.due_seek());
            lengths.push(chunk.len % 2);
            filled.extend_from_slice(&chunk.samples[..chunk.len]);
        }
        assert!(lengths.iter().all(|odd| *odd == 0));
        assert_eq!(filled.len(), expected.len());
        assert!(filled == expected);
    }

    pub(crate) fn corrupt_file(bad_frames: usize, good_after: usize) -> NamedTempFile {
        let good = [&[0xFF, 0xFB, 0x90, 0xC0][..], &[0; 413]].concat();
        let bad = [
            &[0xFF, 0xFB, 0x90, 0xC0, 0, 0, 0, 0, 0, 0x01, 0xE0][..],
            &[0; 406],
        ]
        .concat();
        let mut file = tempfile::Builder::new().suffix(".mp3").tempfile().unwrap();
        let frames = std::iter::repeat_n(&good, 4)
            .chain(std::iter::repeat_n(&bad, bad_frames))
            .chain(std::iter::repeat_n(&good, good_after));
        for frame in frames {
            file.write_all(frame).unwrap();
        }
        file
    }

    #[test]
    fn a_short_run_of_undecodable_packets_is_skipped_and_play_goes_on() {
        let file = corrupt_file(3, 4);
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(4);
        let (_source, mut feed) = fed(&file, callback_sender);
        let mut chunk = feed.empty_consumer.pop().unwrap();
        let mut filled = 0;
        while chunk.mark == ChunkMark::Samples {
            feed.fill(&mut chunk, feed.due_seek());
            filled += chunk.len;
        }
        assert_eq!(filled, 8 * 1_152);
        assert!(matches!(
            callback_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }

    #[test]
    fn a_run_of_undecodable_packets_ends_the_feed_and_reports_corrupt() {
        let file = corrupt_file(24, 0);
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(4);
        let (_source, mut feed) = fed(&file, callback_sender);
        let mut chunk = feed.empty_consumer.pop().unwrap();
        feed.fill(&mut chunk, feed.due_seek());
        assert_eq!(chunk.mark, ChunkMark::End);
        assert!(matches!(
            callback_receiver.try_recv(),
            Ok(AudioMessage::Engine(EngineMessage::Interrupted(
                revision,
                AudioError::Decode {
                    path,
                    error: DecodeError::Corrupt,
                },
            ))) if path == file.path() && revision == feed.revision
        ));
    }

    #[test]
    fn a_refused_seek_plays_on_from_the_feeder_position_to_the_end() {
        let file = corrupt_file(24, 0);
        let (mut source, mut feed) = opened(&file);
        file.as_file().set_len(417 * 4).unwrap();
        source.seek(Duration::from_millis(500));
        feed.prime();
        assert_eq!((feed.sought, feed.generation), (1, 0));
        let bound = 1_000_000;
        assert!(pulled(&mut source, bound).len() < bound);
        assert_eq!(pulled(&mut source, 1), []);
    }
}
