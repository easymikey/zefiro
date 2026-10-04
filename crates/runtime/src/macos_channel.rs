#![cfg(target_os = "macos")]

use ::macos::message::MacosMessage;
use crossbeam_channel::{Receiver, Sender, bounded};

const HEARD: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct MacosChannel {
    pub(crate) sender: Sender<MacosMessage>,
    pub(crate) receiver: Receiver<MacosMessage>,
}

impl MacosChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = bounded(HEARD);
        Self { sender, receiver }
    }
}
