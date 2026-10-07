use std::{
    any::Any,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, never, select_biased};
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
        let silent = never();
        let events = file_stream.events().unwrap_or(&silent);
        let timeout = deadline_at.map_or(Duration::MAX, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        select_biased! {
            recv(self.cmd_receiver) -> first => {
                first.map_or(LoopInput::Closed, |first| {
                    LoopInput::Message(M::from(gather(first, self.cmd_receiver)))
                })
            }
            recv(self.callback_receiver) -> message => {
                message.map_or(LoopInput::Lost(WaitSource::Callback), LoopInput::Message)
            }
            recv(self.finished_receiver) -> finished => {
                match finished {
                    Ok(Ok(message)) => LoopInput::Message(message),
                    Ok(Err(payload)) => LoopInput::Panicked(payload),
                    Err(_) => LoopInput::Lost(WaitSource::Finished),
                }
            }
            recv(events) -> change => {
                file_stream
                    .changed(change)
                    .map_or(LoopInput::Lost(WaitSource::Files), LoopInput::Message)
            }
            default(timeout) => LoopInput::Due,
        }
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
