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
        state::{Closed, Engine, EngineState},
    },
};

pub const AUDIO_EXTENSIONS: &[&str] =
    &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

pub struct AudioDriver {
    engine: Engine,
    deck: Deck,
}

impl std::fmt::Debug for AudioDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioDriver")
            .field("engine", &self.engine)
            .finish_non_exhaustive()
    }
}

impl AudioDriver {
    #[must_use]
    pub fn new(
        settings: AudioSettings,
        callback_sender: Sender<AudioMessage>,
    ) -> (Self, tap::SpectrumTap) {
        let (spectrum_buffers, spectrum_tap) = tap::spectrum_channel();
        let driver = Self {
            engine: Engine::new(EngineState::Closed(Closed {
                settings,
                track_load: None,
                speed: Speed::default(),
            })),
            deck: Deck::new(spectrum_buffers, callback_sender),
        };
        (driver, spectrum_tap)
    }
}
