#![forbid(unsafe_code)]

mod config;
mod deck;
mod device;
mod engine;
mod error;
mod spectrum;
mod tap;

pub use config::{EngineConfig, UnityVolume};
use crossbeam_channel::Receiver as CmdReceiver;
pub use error::{DeviceError, Error};
use kernel::{AudioCmd, AudioEvent, Outbox, Refusals};
pub use spectrum::SpectrumAnalyzer;
pub use tap::SpectrumTap;

use crate::{
    deck::Deck,
    engine::thread::{EngineThread, audio_thread, start},
    tap::Handoff,
};

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

    pub fn run<O: Outbox<AudioEvent> + Refusals>(
        self,
        commands: &CmdReceiver<AudioCmd>,
        outbox: &O,
    ) {
        let AudioLoop { config, spectrum } = self;
        let mut deck = Deck::new(spectrum);
        let state = start(config, &mut deck);
        audio_thread(
            commands,
            EngineThread {
                engine: state,
                deck,
            },
            outbox,
        );
    }
}
