#![cfg(target_os = "macos")]

use ::macos::{CoverReader, MacosLoop};
use crossbeam_channel::{Receiver, Sender};
use kernel::{MacosCmd, MacosEvent, Message, domain::Driver};

use crate::{
    driver::{DriverThread, spawn_driver},
    error::Error,
    registry,
    sender::DriverSender,
};

pub(crate) fn spawn(
    read_cover: CoverReader,
    sender: &Sender<Message>,
) -> Result<DriverThread<MacosCmd>, Error> {
    spawn_driver(
        registry::row(Driver::Macos),
        move |inbox: &Receiver<MacosCmd>, outbox: &DriverSender<MacosEvent>| {
            MacosLoop::new(read_cover).run(inbox, outbox);
        },
        sender,
    )
}
