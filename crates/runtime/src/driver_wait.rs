use std::time::Instant;

use crossbeam_channel::{Receiver, Select, never};
use kernel::cmd::Cmds;

use crate::watcher::FileStream;

pub(crate) struct Inboxes<'a, C, M> {
    pub(crate) commands: &'a Receiver<C>,
    pub(crate) heard: Receiver<M>,
    pub(crate) finished: Receiver<M>,
}

pub(crate) enum WaitSource {
    Heard,
    Finished,
    Files,
}

pub(crate) enum LoopInput<M> {
    Heard(M),
    Lost(WaitSource),
    Due,
    Closed,
}

impl<C, M: From<Cmds<C>>> Inboxes<'_, C, M> {
    pub(crate) fn wait(
        &self,
        files: &FileStream<M>,
        deadline: Option<Instant>,
    ) -> LoopInput<M> {
        let mut select = Select::new();
        let commands = select.recv(self.commands);
        let heard = select.recv(&self.heard);
        let finished = select.recv(&self.finished);
        let silent = never();
        let events = files.events().unwrap_or(&silent);
        let changes = select.recv(events);
        let selected = match deadline {
            Some(deadline) => select.select_deadline(deadline),
            None => Ok(select.select()),
        };
        let Ok(operation) = selected else {
            return LoopInput::Due;
        };
        let index = operation.index();
        if index == commands {
            return operation
                .recv(self.commands)
                .map_or(LoopInput::Closed, |first| {
                    LoopInput::Heard(M::from(gather(first, self.commands)))
                });
        }
        if index == heard {
            return operation
                .recv(&self.heard)
                .map_or(LoopInput::Lost(WaitSource::Heard), LoopInput::Heard);
        }
        if index == finished {
            return operation
                .recv(&self.finished)
                .map_or(LoopInput::Lost(WaitSource::Finished), LoopInput::Heard);
        }
        if index == changes {
            return files
                .heard(operation.recv(events))
                .map_or(LoopInput::Lost(WaitSource::Files), LoopInput::Heard);
        }
        LoopInput::Closed
    }

    pub(crate) fn lose(&mut self, source: &WaitSource, files: &mut FileStream<M>) {
        match source {
            WaitSource::Heard => self.heard = never(),
            WaitSource::Finished => self.finished = never(),
            WaitSource::Files => files.lose(),
        }
    }
}

fn gather<C>(first: C, commands: &Receiver<C>) -> Cmds<C> {
    let mut cmds = vec![first];
    cmds.extend(commands.try_iter());
    Cmds {
        cmds,
        at: Instant::now(),
    }
}
