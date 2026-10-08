use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{Ordering, fence},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError};
use kernel::cmd::Playback;

use crate::deck::feed::{Feed, FeedCmd, Wake};

const FEED_PERIOD: Duration = Duration::from_millis(20);
const FEED_LINGER: Duration = Duration::from_millis(100);

pub(crate) fn serve(receiver: &Receiver<FeedCmd>) {
    let mut feeds: Vec<Feed> = Vec::new();
    let mut inbox = Some(receiver);
    let mut playback = Playback::Playing;
    let mut wait = Some(FEED_PERIOD);
    while inbox.is_some() || !feeds.is_empty() {
        let received = match (inbox, wait) {
            (Some(open), Some(timeout)) if !feeds.is_empty() => {
                open.recv_timeout(timeout)
            }
            (Some(open), _) => open
                .recv()
                .map_err(|RecvError| RecvTimeoutError::Disconnected),
            (None, _) => {
                thread::sleep(FEED_PERIOD);
                Err(RecvTimeoutError::Timeout)
            }
        };
        wait = received.is_ok().then_some(FEED_LINGER);
        match received {
            Ok(FeedCmd::Serve(feed)) => feeds.push(*feed),
            Ok(FeedCmd::Pace(next_playback)) => playback = next_playback,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => inbox = None,
        }
        if playback == Playback::Playing {
            wait = Some(FEED_PERIOD);
        }
        feeds.retain_mut(|feed| {
            if feed.empty_consumer.is_abandoned() {
                fence(Ordering::Acquire);
                feed.wake();
                return feed.wake == Wake::Pending;
            }
            feed.wake();
            feed.buffer();
            catch_unwind(AssertUnwindSafe(|| feed.serve())).is_ok()
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::TryRecvError;
    use kernel::{cmd::Playback, domain::revision::Revision};

    use crate::{
        deck::{
            event::DeckEvent,
            feed::{
                CHUNK_COUNT,
                FeedCmd,
                feed_channel,
                play,
                serve::{FEED_PERIOD, serve},
                tests::{corrupt_file, fed, opened, pulled},
            },
            source::{
                DecodedTrack,
                decode,
                tests::{decoded, ramp_file},
            },
        },
        engine::message::AudioMessage,
    };

    #[test]
    fn serve_keeps_two_sources_supplied_and_returns_once_they_are_gone() {
        let first = ramp_file(2, 70_000);
        let second = ramp_file(1, 150_000);
        let (feed_sender, feed_receiver) = crossbeam_channel::bounded(4);
        let (done_sender, done_receiver) = crossbeam_channel::bounded(1);
        thread::scope(|scope| {
            scope.spawn(|| {
                serve(&feed_receiver);
                done_sender.send(()).unwrap();
            });
            let (mut first_source, first_feed) = opened(&first);
            let (mut second_source, second_feed) = opened(&second);
            feed_sender
                .send(FeedCmd::Serve(Box::new(first_feed)))
                .unwrap();
            feed_sender
                .send(FeedCmd::Serve(Box::new(second_feed)))
                .unwrap();
            let mut first_pulled = Vec::new();
            let mut second_pulled = Vec::new();
            loop {
                let first_block = pulled(&mut first_source, 64);
                let second_block = pulled(&mut second_source, 64);
                if first_block.is_empty() && second_block.is_empty() {
                    break;
                }
                first_pulled
                    .extend(first_block.into_iter().filter(|sample| *sample != 0.0));
                second_pulled
                    .extend(second_block.into_iter().filter(|sample| *sample != 0.0));
            }
            let first_expected = decoded(&first);
            let second_expected = decoded(&second);
            assert_eq!(
                (first_pulled.len(), second_pulled.len()),
                (first_expected.len(), second_expected.len())
            );
            assert!(first_pulled == first_expected && second_pulled == second_expected);
            drop(first_source);
            drop(second_source);
            drop(feed_sender);
            assert_eq!(done_receiver.recv_timeout(Duration::from_secs(10)), Ok(()));
        });
    }

    #[test]
    fn a_dropped_source_gets_no_more_chunks() {
        let file = corrupt_file(24, 0);
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(4);
        let (source, feed) = fed(&file, callback_sender);
        let (feed_sender, feed_receiver) = crossbeam_channel::bounded(1);
        drop(source);
        feed_sender.send(FeedCmd::Serve(Box::new(feed))).unwrap();
        drop(feed_sender);
        serve(&feed_receiver);
        assert!(matches!(
            callback_receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn a_raised_signal_reaches_the_driver_through_the_feeder() {
        let file = ramp_file(1, 100);
        let revision = Revision::default().next();
        thread::scope(|scope| {
            let (callback_sender, callback_receiver) = crossbeam_channel::bounded(1);
            let deck_event = DeckEvent::Woke(Revision::default());
            deck_event.wake(&callback_sender).unwrap();
            let (feed_sender, feed_receiver) = crossbeam_channel::bounded(4);
            scope.spawn(move || serve(&feed_receiver));
            let decoder = decode(file.path()).unwrap();
            let decoded_track = DecodedTrack { revision, decoder };
            let (mut source, feed) = feed_channel(decoded_track, 1, callback_sender);
            let (mut envelope, _control) = play(&source, feed, &feed_sender);
            let pulled = crate::deck::tests::pulled(&mut source, &mut envelope, 200);
            assert_eq!(pulled.len(), 100);
            thread::sleep(3 * FEED_PERIOD);
            assert!(callback_receiver.try_recv().is_ok());
            assert!(matches!(
                callback_receiver.recv_timeout(Duration::from_secs(1)),
                Ok(AudioMessage::Deck(DeckEvent::Woke(woke))) if woke == revision
            ));
        });
    }

    #[test]
    fn a_paused_feeder_waits_for_its_next_command() {
        let file = ramp_file(1, 40_000);
        thread::scope(|scope| {
            let (feed_sender, feed_receiver) = crossbeam_channel::bounded(4);
            scope.spawn(move || serve(&feed_receiver));
            let (mut source, feed) = opened(&file);
            let chunk_len = feed.empty_consumer.peek().unwrap().samples.len();
            feed_sender.send(FeedCmd::Serve(Box::new(feed))).unwrap();
            feed_sender.send(FeedCmd::Pace(Playback::Paused)).unwrap();
            while source.full_consumer.slots() < CHUNK_COUNT {
                thread::yield_now();
            }
            thread::sleep(10 * FEED_PERIOD);
            assert_eq!(pulled(&mut source, chunk_len + 1).len(), chunk_len + 1);
            thread::sleep(5 * FEED_PERIOD);
            assert_eq!(source.full_consumer.slots(), CHUNK_COUNT - 2);
            feed_sender.send(FeedCmd::Pace(Playback::Playing)).unwrap();
            thread::sleep(3 * FEED_PERIOD);
            assert_eq!(source.full_consumer.slots(), CHUNK_COUNT - 1);
        });
    }
}
