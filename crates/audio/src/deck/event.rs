use kernel::domain::OutputDevice;

use crate::{
    deck::source::TrackDecoder,
    error::{AudioError, DeviceError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Ticket(u64);

impl Ticket {
    pub(crate) fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

pub(crate) enum DeckEvent {
    Fault(rodio::cpal::StreamError),
    Decoded {
        ticket: Ticket,
        outcome: Result<TrackDecoder, AudioError>,
    },
    Preloaded {
        ticket: Ticket,
        outcome: Result<TrackDecoder, AudioError>,
    },
    DevicesListed(Result<Vec<OutputDevice>, DeviceError>),
    Track(Ticket),
}
