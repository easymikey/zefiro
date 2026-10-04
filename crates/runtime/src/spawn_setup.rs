use crossbeam_channel::Sender;
use kernel::{domain::settings::AudioSettings, message::Message};

#[cfg(target_os = "macos")] use crate::macos_channel::MacosChannel;
use crate::{latest::LatestSenders, startup_paths::StartupPaths};

#[derive(Debug)]
pub(crate) struct SpawnSetup<'a> {
    pub(crate) audio: &'a AudioSettings,
    pub(crate) paths: &'a StartupPaths,
    pub(crate) inbox: &'a Sender<Message>,
    pub(crate) writers: &'a LatestSenders,
    #[cfg(target_os = "macos")]
    pub(crate) macos: &'a MacosChannel,
}
