#[cfg(target_os = "macos")] use ::macos::message::MacosMessage;
use config::driver::paths::ConfigPaths;
use crossbeam_channel::Sender;
#[cfg(target_os = "macos")] use crossbeam_channel::{Receiver, bounded};
use kernel::{domain::settings::AudioSettings, message::Message};
use library::dirs::LibraryDirs;

use crate::latest::LatestSenders;

#[cfg(target_os = "macos")]
const HEARD: usize = 64;

#[derive(Debug, Clone)]
pub struct StartupPaths {
    pub config: ConfigPaths,
    pub library: LibraryDirs,
}

#[derive(Debug)]
pub(crate) struct SpawnSetup<'a> {
    pub(crate) audio: &'a AudioSettings,
    pub(crate) paths: &'a StartupPaths,
    pub(crate) inbox: &'a Sender<Message>,
    pub(crate) writers: &'a LatestSenders,
    #[cfg(target_os = "macos")]
    pub(crate) macos: &'a MacosChannel,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub(crate) struct MacosChannel {
    pub(crate) sender: Sender<MacosMessage>,
    pub(crate) receiver: Receiver<MacosMessage>,
}

#[cfg(target_os = "macos")]
impl MacosChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = bounded(HEARD);
        Self { sender, receiver }
    }
}
