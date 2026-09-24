#![forbid(unsafe_code)]

mod config;
mod deck;
mod device;
mod engine;
mod error;
mod spectrum;
mod tap;

use std::sync::Arc;

pub use config::{EngineConfig, UnityVolume};
use crossbeam_channel::{Receiver as CmdReceiver, Sender as EventSender};
pub use error::{AudioError, DeviceError};
use kernel::{AudioCmd, Message};
pub use spectrum::SpectrumAnalyzer;
pub use tap::SpectrumTap;

use crate::{
    deck::Deck,
    engine::thread::{Worker, audio_thread, boot},
    tap::Ring,
};

pub const DECODABLE_EXTENSIONS: &[&str] =
    &["flac", "mp3", "mp4", "m4a", "m4b", "ogg", "wav", "mkv"];

#[derive(Debug)]
pub struct AudioLoop {
    config: EngineConfig,
    ring: Arc<Ring>,
}

#[must_use]
pub fn prepare(config: EngineConfig) -> (AudioLoop, SpectrumTap) {
    let (ring, spectrum) = tap::new_tap();
    (AudioLoop { config, ring }, spectrum)
}

impl AudioLoop {
    pub fn run(self, commands: &CmdReceiver<AudioCmd>, mailbox: &EventSender<Message>) {
        let AudioLoop { config, ring } = self;
        let mut deck = Deck::new(mailbox.clone(), ring);
        let state = boot(config, &mut deck);
        audio_thread(commands, state, Worker { deck });
    }
}
