use std::{
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
};

use crossbeam_channel::Receiver;
use kernel::domain::revision::Revision;

use crate::{
    deck::{
        feed::{FeedCmd, serve},
        source::{TrackDecoder, decode},
    },
    device::list_output_devices,
    engine::message::AudioMessage,
    error::{Error, list_devices_error},
};

#[derive(Debug)]
pub enum AudioJob {
    Decode { path: PathBuf, revision: Revision },
    Preload { path: PathBuf, revision: Revision },
    ListDevices,
    Feed(Receiver<FeedCmd>),
}

impl AudioJob {
    #[must_use]
    pub fn run(self) -> AudioMessage {
        match self {
            AudioJob::Decode { path, revision } => {
                let result = decode_caught(path);
                AudioMessage::Decoded { revision, result }
            }
            AudioJob::Preload { path, revision } => {
                let result = decode_caught(path);
                AudioMessage::Preloaded { revision, result }
            }
            AudioJob::ListDevices => AudioMessage::DevicesListed(
                list_output_devices().map_err(|error| list_devices_error(&error)),
            ),
            AudioJob::Feed(receiver) => {
                serve(&receiver);
                AudioMessage::Fed
            }
        }
    }
}

fn decode_caught(path: PathBuf) -> Result<TrackDecoder, Error> {
    panic::catch_unwind(AssertUnwindSafe(|| decode(&path)))
        .unwrap_or_else(|_panic| Err(Error::WorkerPanicked(path)))
}

#[cfg(test)]
mod tests {
    use kernel::update::machine::{LoopEffect, Machine, Unhandled};
    use rstest::rstest;

    use crate::{
        deck::job::AudioJob,
        engine::{
            message::AudioMessage,
            state::EngineState,
            tests::{closed, driver_with, live, playing},
        },
    };

    #[test]
    fn a_feed_job_whose_sender_dropped_answers_fed() {
        let (feed_sender, feed_receiver) = crossbeam_channel::bounded(1);
        drop(feed_sender);

        let answer = AudioJob::Feed(feed_receiver).run();

        assert!(matches!(answer, AudioMessage::Fed));
    }

    #[rstest]
    #[case::closed(closed())]
    #[case::live(EngineState::Live(live()))]
    #[case::playing(EngineState::Live(playing()))]
    fn the_feeder_starts_once_as_a_feed_job_and_its_end_is_handled_in_every_state(
        #[case] engine_state: EngineState,
    ) {
        let mut driver = driver_with(engine_state);

        let (started, started_events) = driver
            .transition(AudioMessage::Started)
            .unwrap()
            .into_parts();
        let restarted = driver.transition(AudioMessage::Started);
        let (fed, fed_events) =
            driver.transition(AudioMessage::Fed).unwrap().into_parts();

        assert!(matches!(
            started.as_slice(),
            [LoopEffect::Run(AudioJob::Feed(_))]
        ));
        assert!(started_events.is_empty());
        assert_eq!(restarted.err(), Some(Unhandled));
        assert!(fed.is_empty());
        assert!(fed_events.is_empty());
    }
}
