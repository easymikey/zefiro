#![forbid(unsafe_code)]

pub mod deck;
mod device;
pub mod engine;
mod error;
pub(crate) mod gain;
pub mod spectrum;
pub mod tap;

use crossbeam_channel::Sender;
use kernel::domain::{settings::AudioSettings, speed::Speed};

use crate::{
    deck::Deck,
    engine::{
        message::AudioMessage,
        revisions::JobRevisions,
        state::{Closed, Engine},
    },
};

pub const DECODABLE_EXTENSIONS: &[&str] =
    &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

pub struct AudioDriver {
    engine: Engine,
    revisions: JobRevisions,
    deck: Deck,
}

impl std::fmt::Debug for AudioDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioDriver")
            .field("engine", &self.engine)
            .field("revisions", &self.revisions)
            .finish_non_exhaustive()
    }
}

impl AudioDriver {
    #[must_use]
    pub fn new(
        settings: AudioSettings,
        sender: Sender<AudioMessage>,
    ) -> (Self, tap::SpectrumTap) {
        let (spectrum, tap) = tap::new_tap();
        let driver = Self {
            engine: Engine::Closed(Closed {
                settings,
                pending: None,
                speed: Speed::default(),
            }),
            revisions: JobRevisions::default(),
            deck: Deck::new(spectrum, sender),
        };
        (driver, tap)
    }
}
