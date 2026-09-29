#![cfg(target_os = "macos")]

use ::macos::{CoverReader, SystemLoop};
use crossbeam_channel::{Receiver, Sender};
use kernel::{Message, SystemCmd, SystemEvent, domain::Driver};

use crate::{
    driver::{DriverLoop, DriverThread, spawn_loop},
    error::RuntimeError,
    mailbox::Mailbox,
    registry,
};

#[derive(Debug)]
pub(crate) struct SystemStart(CoverReader);

impl SystemStart {
    pub(crate) fn new(read_cover: CoverReader) -> Self {
        Self(read_cover)
    }
}

impl DriverLoop<SystemCmd, SystemEvent> for SystemStart {
    fn run(self, inbox: &Receiver<SystemCmd>, outbox: &Mailbox<SystemEvent>) {
        SystemLoop::new(self.0).run(inbox, outbox);
    }
}

pub(crate) fn spawn<M>(
    system: M,
    mailbox: &Sender<Message>,
) -> Result<DriverThread<SystemCmd>, RuntimeError>
where
    M: DriverLoop<SystemCmd, SystemEvent>,
{
    spawn_loop(registry::row(Driver::Macos), system, mailbox)
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
