use kernel::domain::ListedDevice;

use crate::{
    deck::source::TrackDecoder,
    error::{DeviceError, Error},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Ticket(u64);

impl Ticket {
    pub(crate) fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

pub(crate) enum DeckEvent {
    OutputLost(rodio::cpal::StreamError),
    Decoded {
        ticket: Ticket,
        outcome: Result<TrackDecoder, Error>,
    },
    Preloaded {
        ticket: Ticket,
        outcome: Result<TrackDecoder, Error>,
    },
    DevicesListed(Result<Vec<ListedDevice>, DeviceError>),
    Track(Ticket),
}
