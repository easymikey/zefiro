use std::{
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
};

use crate::{
    deck::{
        DeckEvent,
        Revision,
        source::{TrackDecoder, decode},
    },
    device::list_output_devices,
    engine::effect::AudioMessage,
    error::{Error, device_error},
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioJob {
    Decode { path: PathBuf, revision: Revision },
    Preload { path: PathBuf, revision: Revision },
    ListDevices,
}

impl AudioJob {
    #[must_use]
    pub fn run(self) -> AudioMessage {
        match self {
            AudioJob::Decode { path, revision } => {
                let result = decode_caught(path);
                AudioMessage::Deck(DeckEvent::Decoded { revision, result })
            }
            AudioJob::Preload { path, revision } => {
                let result = decode_caught(path);
                AudioMessage::Deck(DeckEvent::Preloaded { revision, result })
            }
            AudioJob::ListDevices => AudioMessage::Deck(DeckEvent::DevicesListed(
                list_output_devices().map_err(device_error),
            )),
        }
    }
}

fn decode_caught(path: PathBuf) -> Result<TrackDecoder, Error> {
    panic::catch_unwind(AssertUnwindSafe(|| decode(&path)))
        .unwrap_or_else(|_panic| Err(Error::WorkerPanicked(path)))
}
