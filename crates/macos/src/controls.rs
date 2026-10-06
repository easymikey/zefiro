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
        callback_sender: &Sender<MacosMessage>,
    ) -> Self {
        let center = ffi::shared_command_center();
        let commands = ffi::remote_commands(&center);
        let targets = commands
            .into_iter()
            .map(|(command, trigger)| {
                let handler =
                    RcBlock::new(command_handler(trigger, callback_sender.clone()));
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
    callback_sender: Sender<MacosMessage>,
) -> impl Fn(NonNull<MPRemoteCommandEvent>) -> MPRemoteCommandHandlerStatus + 'static {
    move |event| {
        ffi::borrow_command_event(event, |event| {
            RemoteInput::parse(trigger, event)
                .map_or(MPRemoteCommandHandlerStatus::CommandFailed, |input| {
                    status(callback_sender.try_send(MacosMessage::Remote(input)))
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
    enum ChannelState {
        Room,
        Full,
        Closed,
    }

    #[rstest]
    #[case::room(ChannelState::Room, MPRemoteCommandHandlerStatus::Success)]
    #[case::full(ChannelState::Full, MPRemoteCommandHandlerStatus::CommandFailed)]
    #[case::closed(ChannelState::Closed, MPRemoteCommandHandlerStatus::CommandFailed)]
    fn the_command_status_follows_the_send_result(
        #[case] channel_state: ChannelState,
        #[case] expected: MPRemoteCommandHandlerStatus,
    ) {
        let (callback_sender, receiver) = bounded(1);
        match channel_state {
            ChannelState::Room => {}
            ChannelState::Full => {
                callback_sender.try_send(MacosMessage::Started).unwrap();
            }
            ChannelState::Closed => drop(receiver),
        }
        assert_eq!(
            status(callback_sender.try_send(MacosMessage::Started)),
            expected
        );
    }
}
