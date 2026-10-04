#![forbid(unsafe_code)]

use std::ptr::NonNull;

use block2::RcBlock;
use crossbeam_channel::{Sender, TrySendError};
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_media_player::{
    MPRemoteCommand,
    MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};

use crate::{
    ffi::{self, Trigger},
    message::MacosMessage,
    remote_input::RemoteInput,
};

#[derive(Debug)]
pub(crate) struct Controls {
    targets: Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>,
}

impl Controls {
    #[must_use]
    pub(crate) fn attach(
        _main_thread: MainThreadMarker,
        heard: &Sender<MacosMessage>,
    ) -> Self {
        let center = ffi::shared_command_center();
        let commands = ffi::remote_commands(&center);
        let targets = commands
            .into_iter()
            .map(|(command, trigger)| {
                let handler = RcBlock::new(command_handler(trigger, heard.clone()));
                ffi::enable_command(&command);
                let target = ffi::add_command_target(&command, &handler);
                (command, target)
            })
            .collect();
        Self { targets }
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        self.targets.iter().for_each(|(command, target)| {
            ffi::remove_command_target(command, target);
        });
    }
}

fn command_handler(
    trigger: Trigger,
    heard: Sender<MacosMessage>,
) -> impl Fn(NonNull<MPRemoteCommandEvent>) -> MPRemoteCommandHandlerStatus + 'static {
    move |event| {
        ffi::borrow_command_event(event, |event| {
            RemoteInput::parse(trigger, event)
                .map_or(MPRemoteCommandHandlerStatus::CommandFailed, |input| {
                    status(heard.try_send(MacosMessage::Remote(input)))
                })
        })
    }
}

fn status(
    sent: Result<(), TrySendError<MacosMessage>>,
) -> MPRemoteCommandHandlerStatus {
    sent.map_or(MPRemoteCommandHandlerStatus::CommandFailed, |()| {
        MPRemoteCommandHandlerStatus::Success
    })
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;
    use objc2_media_player::MPRemoteCommandHandlerStatus;
    use rstest::rstest;

    use crate::{controls::status, message::MacosMessage};

    #[derive(Debug, Clone, Copy)]
    enum Queue {
        Room,
        Full,
        Closed,
    }

    #[rstest]
    #[case::room(Queue::Room, MPRemoteCommandHandlerStatus::Success)]
    #[case::full(Queue::Full, MPRemoteCommandHandlerStatus::CommandFailed)]
    #[case::closed(Queue::Closed, MPRemoteCommandHandlerStatus::CommandFailed)]
    fn the_command_status_follows_the_send_result(
        #[case] queue: Queue,
        #[case] expected: MPRemoteCommandHandlerStatus,
    ) {
        let (heard, receiver) = bounded(1);
        match queue {
            Queue::Room => {}
            Queue::Full => heard.try_send(MacosMessage::Started).unwrap(),
            Queue::Closed => drop(receiver),
        }
        assert_eq!(status(heard.try_send(MacosMessage::Started)), expected);
    }
}
