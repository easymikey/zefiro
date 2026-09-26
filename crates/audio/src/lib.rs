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
pub use error::{AudioError, DeviceError};
use kernel::{AudioCmd, AudioEvent, Outbox};
pub use spectrum::SpectrumAnalyzer;
pub use tap::SpectrumTap;

use crate::{
    deck::Deck,
    engine::thread::{Worker, audio_thread, boot},
    tap::Handoff,
};

pub const DECODABLE_EXTENSIONS: &[&str] =
    &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

#[derive(Debug)]
pub struct AudioLoop {
    config: EngineConfig,
    spectrum: Handoff,
}

#[must_use]
pub fn prepare(config: EngineConfig) -> (AudioLoop, SpectrumTap) {
    let (spectrum, tap) = tap::new_tap();
    (AudioLoop { config, spectrum }, tap)
}

impl AudioLoop {
    pub fn run<O: Outbox<AudioEvent>>(
        self,
        commands: &CmdReceiver<AudioCmd>,
        outbox: &O,
    ) {
        let AudioLoop { config, spectrum } = self;
        let mut deck = Deck::new(spectrum);
        let state = boot(config, &mut deck);
        audio_thread(commands, Worker { state, deck }, outbox);
    }
}
