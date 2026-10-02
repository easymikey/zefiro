#![cfg(target_os = "macos")]

use ::macos::{CoverReader, MacosLoop};
use crossbeam_channel::{Receiver, Sender};
use kernel::{MacosCmd, MacosEvent, Message, Outbox, domain::Driver};

use crate::{
    driver::{DriverThread, spawn_driver},
    error::Error,
    registry,
};

pub(crate) fn spawn(
    read_cover: CoverReader,
    sender: &Sender<Message>,
) -> Result<DriverThread<MacosCmd>, Error> {
    spawn_driver(
        registry::row(Driver::Macos),
        move |inbox: &Receiver<MacosCmd>, outbox: &Outbox<MacosEvent>| {
            MacosLoop::new(read_cover).run(inbox, outbox);
        },
        sender,
    )
}
