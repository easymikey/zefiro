use std::{any::Any, time::Instant};

use crossbeam_channel::{Receiver, Select, never};
use kernel::cmd::Cmds;

use crate::watcher::FileStream;

pub(crate) struct WaitSources<'a, C, M> {
    pub(crate) cmd_receiver: &'a Receiver<C>,
    pub(crate) callback_receiver: Receiver<M>,
    pub(crate) finished_receiver: Receiver<Result<M, Box<dyn Any + Send>>>,
}

pub(crate) enum WaitSource {
    Callback,
    Finished,
    Files,
}

pub(crate) enum LoopInput<M> {
    Message(M),
    Panicked(Box<dyn Any + Send>),
    Lost(WaitSource),
    Due,
    Closed,
}

impl<C, M: From<Cmds<C>>> WaitSources<'_, C, M> {
    pub(crate) fn wait(
        &self,
        file_stream: &FileStream<M>,
        deadline_at: Option<Instant>,
    ) -> LoopInput<M> {
        let mut select = Select::new();
        let cmd_index = select.recv(self.cmd_receiver);
        let callback_index = select.recv(&self.callback_receiver);
        let finished = select.recv(&self.finished_receiver);
        let silent = never();
        let events = file_stream.events().unwrap_or(&silent);
        let changes = select.recv(events);
        let selected = match deadline_at {
            Some(deadline) => select.select_deadline(deadline),
            None => Ok(select.select()),
        };
        let Ok(operation) = selected else {
            return LoopInput::Due;
        };
        let index = operation.index();
        if index == cmd_index {
            return operation
                .recv(self.cmd_receiver)
                .map_or(LoopInput::Closed, |first| {
                    LoopInput::Message(M::from(gather(first, self.cmd_receiver)))
                });
        }
        if index == callback_index {
            return operation
                .recv(&self.callback_receiver)
                .map_or(LoopInput::Lost(WaitSource::Callback), LoopInput::Message);
        }
        if index == finished {
            return match operation.recv(&self.finished_receiver) {
                Ok(Ok(message)) => LoopInput::Message(message),
                Ok(Err(payload)) => LoopInput::Panicked(payload),
                Err(_) => LoopInput::Lost(WaitSource::Finished),
            };
        }
        if index == changes {
            return file_stream
                .changed(operation.recv(events))
                .map_or(LoopInput::Lost(WaitSource::Files), LoopInput::Message);
        }
        LoopInput::Closed
    }

    pub(crate) fn lose(
        &mut self,
        source: &WaitSource,
        file_stream: &mut FileStream<M>,
    ) -> Option<M> {
        match source {
            WaitSource::Callback => {
                self.callback_receiver = never();
                None
            }
            WaitSource::Finished => {
                self.finished_receiver = never();
                None
            }
            WaitSource::Files => file_stream.lose(),
        }
    }
}

fn gather<C>(first: C, cmd_receiver: &Receiver<C>) -> Cmds<C> {
    let mut cmds = vec![first];
    cmds.extend(cmd_receiver.try_iter());
    Cmds {
        cmds,
        at: Instant::now(),
    }
}
