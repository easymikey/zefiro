#![forbid(unsafe_code)]

mod deck;
mod device;
mod engine;
mod error;
mod spectrum;
mod tap;

use crossbeam_channel::Sender;
pub use deck::AudioJob;
pub use engine::effect::EngineEffect;
use kernel::domain::{AudioSettings, Speed};
pub use spectrum::SpectrumAnalyzer;
pub use tap::SpectrumTap;

use crate::{
    deck::Deck,
    engine::{
        effect::AudioMessage,
        revisions::Revisions,
        state::{Engine, Muted},
    },
};

pub const DECODABLE_EXTENSIONS: &[&str] =
    &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

pub struct AudioDriver {
    engine: Engine,
    revisions: Revisions,
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
        config: AudioSettings,
        sender: Sender<AudioMessage>,
    ) -> (Self, SpectrumTap) {
        let (spectrum, tap) = tap::new_tap();
        let driver = Self {
            engine: Engine::Muted(Muted {
                settings: config,
                pending: None,
                speed: Speed::default(),
            }),
            revisions: Revisions::default(),
            deck: Deck::new(spectrum, sender),
        };
        (driver, tap)
    }
}
