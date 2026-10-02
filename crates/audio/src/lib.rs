#![forbid(unsafe_code)]

mod config;
mod deck;
mod device;
mod engine;
mod error;
mod spectrum;
mod tap;

pub use config::EngineConfig;
pub use spectrum::SpectrumAnalyzer;
pub use tap::SpectrumTap;

use crate::tap::Handoff;

pub const DECODABLE_EXTENSIONS: &[&str] =
    &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

#[derive(Debug)]
pub struct AudioLoop {
    config: EngineConfig,
    spectrum: Handoff,
}

impl AudioLoop {
    #[must_use]
    pub fn new(config: EngineConfig) -> (Self, SpectrumTap) {
        let (spectrum, tap) = tap::new_tap();
        (Self { config, spectrum }, tap)
    }
}
