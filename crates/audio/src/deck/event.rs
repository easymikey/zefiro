use std::fmt;

use crossbeam_channel::{Sender, TrySendError};
pub(crate) use kernel::domain::Revision;
use kernel::{AudioError, domain::ListedDevice};

use crate::{deck::source::TrackDecoder, engine::effect::AudioMessage, error::Error};

pub enum DeckEvent {
    OutputLost(rodio::cpal::StreamError),
    Decoded {
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    },
    Preloaded {
        revision: Revision,
        result: Result<TrackDecoder, Error>,
    },
    DevicesListed(Result<Vec<ListedDevice>, AudioError>),
    Track(Revision),
}

impl DeckEvent {
    pub(crate) fn notify(
        self,
        sender: &Sender<AudioMessage>,
    ) -> Result<(), TrySendError<AudioMessage>> {
        match sender.try_send(AudioMessage::Deck(self)) {
            Err(TrySendError::Disconnected(_)) => Ok(()),
            sent => sent,
        }
    }
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
            DeckEvent::Track(revision) => {
                f.debug_tuple("Track").field(revision).finish()
            }
        }
    }
}
