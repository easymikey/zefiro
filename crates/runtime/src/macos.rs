#![cfg(target_os = "macos")]

pub(crate) use ::macos::{Controls, MainThreadMarker};
use ::macos::{CoverReader, SystemLoop};
use crossbeam_channel::{Receiver, Sender};
use kernel::{Message, SystemCmd, domain::Driver};

use crate::{
    driver::{DriverLoop, DriverThread, spawn_loop},
    error::RuntimeError,
};

#[derive(Debug)]
pub struct SystemStart(CoverReader);

impl SystemStart {
    pub(crate) fn new(read_cover: CoverReader) -> Self {
        Self(read_cover)
    }
}

impl DriverLoop<SystemCmd> for SystemStart {
    fn run(self, inbox: &Receiver<SystemCmd>, mailbox: &Sender<Message>) {
        SystemLoop::new(self.0).run(inbox, mailbox);
    }
}

pub(crate) fn spawn<M>(
    system: M,
    mailbox: Sender<Message>,
) -> Result<(Option<Controls>, DriverThread<SystemCmd>), RuntimeError>
where
    M: DriverLoop<SystemCmd>,
{
    let controls =
        MainThreadMarker::new().map(|marker| Controls::attach(marker, &mailbox));
    let thread = spawn_loop(Driver::Macos, system, mailbox)?;
    Ok((controls, thread))
}

pub(crate) fn pump(marker: MainThreadMarker) {
    ::macos::pump_main_run_loop(marker);
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;

    use crate::{driver::NoDriver, macos::spawn};

    #[test]
    fn a_worker_is_reachable_off_the_main_thread() {
        let (mailbox, _reports) = unbounded();

        let (controls, thread) = spawn(NoDriver, mailbox).unwrap();

        assert!(controls.is_none());
        drop(thread.commands);
        thread.handle.join().unwrap().unwrap();
    }
}
