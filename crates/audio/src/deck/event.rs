use std::fmt;

use kernel::{
    domain::{device::ListedDevice, revision::Revision, transport::StreamError},
    message::AudioError,
};

use crate::{deck::source::TrackDecoder, error::Error};

pub enum DeckEvent {
    OutputLost(StreamError),
    Decoded {
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    },
    Preloaded {
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    },
    DevicesListed(Result<Vec<ListedDevice>, AudioError>),
    Woke(Revision),
}

impl fmt::Debug for DeckEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeckEvent::OutputLost(error) => {
                f.debug_tuple("OutputLost").field(error).finish()
            }
            DeckEvent::Decoded { revision, .. } => f
                .debug_struct("Decoded")
                .field("revision", revision)
                .finish_non_exhaustive(),
            DeckEvent::Preloaded { revision, .. } => f
                .debug_struct("Preloaded")
                .field("revision", revision)
                .finish_non_exhaustive(),
            DeckEvent::DevicesListed(listed) => {
                f.debug_tuple("DevicesListed").field(listed).finish()
            }
            DeckEvent::Woke(revision) => f.debug_tuple("Woke").field(revision).finish(),
        }
    }
}
