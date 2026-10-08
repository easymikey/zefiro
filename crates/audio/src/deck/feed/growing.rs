use std::{sync::atomic::Ordering, time::Duration};

use crossbeam_channel::TrySendError;

use crate::deck::{
    event::DeckEvent,
    feed::{Feed, FeedPhase, Wake},
    source::GrowingDownload,
};

const READ_MARGIN: u64 = 256 * 1024;

impl Feed {
    pub(crate) fn due_seek(&self) -> Option<u32> {
        let generation = self.seek_target.generation.load(Ordering::Acquire);
        let target = Duration::from_nanos(
            self.seek_target.position_nanos.load(Ordering::Relaxed),
        );
        let reachable = self
            .decoder
            .download
            .as_ref()
            .zip(self.decoder.duration())
            .is_none_or(|(download, duration)| {
                let byte = u128::from(download.byte_len) * target.as_nanos()
                    / duration.as_nanos().max(1);
                buffered(download, u64::try_from(byte).unwrap_or(u64::MAX))
            });
        (generation != self.sought && reachable).then_some(generation)
    }

    pub(crate) fn has_margin(&self) -> bool {
        self.decoder.download.as_ref().is_none_or(|download| {
            buffered(download, download.read_byte.load(Ordering::Acquire))
        })
    }

    pub(crate) fn buffer(&mut self) {
        if self.phase == FeedPhase::Buffering && self.has_margin() {
            self.phase = FeedPhase::Decoding;
            self.buffering_wake = Wake::Pending;
        }
        let Some(download) = &self.decoder.download else {
            return;
        };
        if self.buffering_wake == Wake::Pending {
            let event = match self.phase {
                FeedPhase::Buffering => DeckEvent::Buffering(download.revision),
                FeedPhase::Decoding | FeedPhase::Drained => {
                    DeckEvent::Buffered(download.revision)
                }
            };
            self.buffering_wake = match event.wake(&self.callback_sender) {
                Ok(()) => Wake::Sent,
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                    Wake::Pending
                }
            };
        }
    }
}

fn buffered(download: &GrowingDownload, from: u64) -> bool {
    let downloaded = download.downloaded.load(Ordering::Acquire);
    downloaded >= download.byte_len || downloaded.saturating_sub(from) >= READ_MARGIN
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use kernel::domain::revision::Revision;

    use crate::deck::{
        feed::{feed_channel, growing::READ_MARGIN},
        source::{
            DecodedTrack,
            GrowingDownload,
            tests::{decoded, ramp_file},
        },
    };

    #[test]
    fn a_growing_feed_waits_below_its_read_margin_and_reports_buffering_until_grown() {
        let file = ramp_file(2, 200_000);
        let byte_len = file.as_file().metadata().unwrap().len();
        let downloaded = Arc::new(AtomicU64::new(READ_MARGIN));
        let download = Revision::default().next();
        let decoder = GrowingDownload {
            downloaded: Arc::clone(&downloaded),
            byte_len,
            revision: download,
            read_byte: Arc::default(),
        }
        .decode(file.path())
        .unwrap();
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(4);
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder,
        };
        let (source, mut feed) = feed_channel(decoded_track, 2, callback_sender);

        feed.prime();
        feed.buffer();
        let stalled = source.full_consumer.slots();
        downloaded.store(byte_len, Ordering::Release);
        feed.buffer();
        feed.prime();
        let reports: Vec<String> = callback_receiver
            .try_iter()
            .map(|audio_message| format!("{audio_message:?}"))
            .collect();

        assert_eq!(stalled, 0);
        assert!(source.full_consumer.slots() > 0);
        assert_eq!(
            reports,
            [
                format!("Deck(Buffering({download:?}))"),
                format!("Deck(Buffered({download:?}))"),
            ]
        );
    }

    #[test]
    fn a_seek_past_the_read_margin_keeps_the_old_audio_until_a_grow_passes_it() {
        let file = ramp_file(1, 400_000);
        let expected = decoded(&file);
        let byte_len = file.as_file().metadata().unwrap().len();
        let downloaded = Arc::new(AtomicU64::new(2 * READ_MARGIN));
        let decoder = GrowingDownload {
            downloaded: Arc::clone(&downloaded),
            byte_len,
            revision: Revision::default().next(),
            read_byte: Arc::default(),
        }
        .decode(file.path())
        .unwrap();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder,
        };
        let (mut source, mut feed) = feed_channel(decoded_track, 1, callback_sender);

        source.seek(Duration::from_secs(30));
        let due_before_grow = feed.due_seek();
        let mut kept = feed.empty_consumer.pop().unwrap();
        feed.fill(&mut kept, due_before_grow);
        downloaded.store(byte_len, Ordering::Release);
        let due_after_grow = feed.due_seek();
        let mut landed = feed.empty_consumer.pop().unwrap();
        feed.fill(&mut landed, due_after_grow);

        assert_eq!((due_before_grow, due_after_grow), (None, Some(1)));
        assert_eq!(
            (kept.generation, kept.samples.first()),
            (0, expected.first())
        );
        assert_eq!(
            (landed.generation, landed.samples.first()),
            (1, expected.get(240_000))
        );
    }
}
