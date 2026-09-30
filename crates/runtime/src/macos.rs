#![cfg(target_os = "macos")]

use ::macos::{CoverReader, MacosLoop};
use crossbeam_channel::{Receiver, Sender};
use kernel::{MacosCmd, MacosEvent, Message, domain::Driver};

use crate::{
    driver::{DriverLoop, DriverThread, spawn_loop},
    error::Error,
    registry,
    sender::DriverSender,
};

#[derive(Debug)]
pub(crate) struct MacosStart(CoverReader);

impl MacosStart {
    pub(crate) fn new(read_cover: CoverReader) -> Self {
        Self(read_cover)
    }
}

impl DriverLoop<MacosCmd, MacosEvent> for MacosStart {
    fn run(self, inbox: &Receiver<MacosCmd>, outbox: &DriverSender<MacosEvent>) {
        MacosLoop::new(self.0).run(inbox, outbox);
    }
}

pub(crate) fn spawn<M>(
    macos: M,
    sender: &Sender<Message>,
) -> Result<DriverThread<MacosCmd>, Error>
where
    M: DriverLoop<MacosCmd, MacosEvent>,
{
    spawn_loop(registry::row(Driver::Macos), macos, sender)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;

    use crate::{driver::NoDriver, macos::spawn};

    #[test]
    fn a_worker_is_reachable_off_the_main_thread() {
        let (mailbox, _reports) = unbounded();

        let thread = spawn(NoDriver, &mailbox).unwrap();

        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }
}
