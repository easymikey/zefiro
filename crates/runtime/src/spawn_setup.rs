#[cfg(target_os = "macos")] use ::macos::message::MacosMessage;
use config::driver::paths::ConfigPaths;
use crossbeam_channel::Sender;
#[cfg(target_os = "macos")] use crossbeam_channel::{Receiver, bounded};
use kernel::{domain::settings::AudioSettings, message::Message};
use library::dirs::LibraryDirs;

use crate::latest::LatestSenders;

pub(crate) const CALLBACK_SLOTS: usize = 64;

#[derive(Debug, Clone)]
pub struct StartupPaths {
    pub config_paths: ConfigPaths,
    pub library_dirs: LibraryDirs,
}

#[derive(Debug)]
pub(crate) struct SpawnSetup<'a> {
    pub(crate) audio_settings: &'a AudioSettings,
    pub(crate) paths: &'a StartupPaths,
    pub(crate) inbox: &'a Sender<Message>,
    pub(crate) latest_senders: &'a LatestSenders,
    #[cfg(target_os = "macos")]
    pub(crate) macos_channel: &'a MacosChannel,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub(crate) struct MacosChannel {
    pub(crate) callback_sender: Sender<MacosMessage>,
    pub(crate) callback_receiver: Receiver<MacosMessage>,
}

#[cfg(target_os = "macos")]
impl MacosChannel {
    pub(crate) fn new() -> Self {
        let (callback_sender, callback_receiver) = bounded(CALLBACK_SLOTS);
        Self {
            callback_sender,
            callback_receiver,
        }
    }
}
