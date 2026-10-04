#![forbid(unsafe_code)]

use std::{ptr::NonNull, time::Duration};

use block2::RcBlock;
use crossbeam_channel::{Sender, TrySendError};
use kernel::message::PlaybackRequest;
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent,
    MPRemoteCommand,
    MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
    MPSeekCommandEvent,
    MPSeekCommandEventType,
};

use crate::{driver::MacosMessage, ffi};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Trigger {
    Press(PlaybackRequest),
    Hold(PlaybackRequest),
    Scrub,
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RemoteInput {
    Press(PlaybackRequest),
    HoldBegan(PlaybackRequest),
    HoldEnded,
    Scrub(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RemoteInputError {
    #[error("the remote command arrived with an event of the wrong kind")]
    WrongEvent,
    #[error("the remote command asked for a negative or invalid position")]
    InvalidPosition,
}

impl RemoteInput {
    pub(crate) fn parse(
        trigger: Trigger,
        event: &MPRemoteCommandEvent,
    ) -> Result<Self, RemoteInputError> {
        match trigger {
            Trigger::Press(request) => Ok(RemoteInput::Press(request)),
            Trigger::Hold(request) => event
                .downcast_ref::<MPSeekCommandEvent>()
                .ok_or(RemoteInputError::WrongEvent)
                .map(|seek| {
                    RemoteInput::from_phase(request, ffi::seek_event_phase(seek))
                }),
            Trigger::Scrub => event
                .downcast_ref::<MPChangePlaybackPositionCommandEvent>()
                .ok_or(RemoteInputError::WrongEvent)
                .and_then(|scrub| {
                    RemoteInput::from_seconds(ffi::scrub_position_seconds(scrub))
                }),
        }
    }

    fn from_phase(request: PlaybackRequest, phase: MPSeekCommandEventType) -> Self {
        if phase == MPSeekCommandEventType::BeginSeeking {
            RemoteInput::HoldBegan(request)
        } else {
            RemoteInput::HoldEnded
        }
    }

    fn from_seconds(seconds: f64) -> Result<Self, RemoteInputError> {
        Duration::try_from_secs_f64(seconds)
            .map(RemoteInput::Scrub)
            .map_err(|_| RemoteInputError::InvalidPosition)
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
    use std::time::Duration;

    use crossbeam_channel::bounded;
    use kernel::message::PlaybackRequest;
    use objc2_media_player::{MPRemoteCommandHandlerStatus, MPSeekCommandEventType};
    use rstest::rstest;

    use crate::{
        controls::{RemoteInput, RemoteInputError, status},
        driver::MacosMessage,
    };

    #[rstest]
    #[case::begin(
        MPSeekCommandEventType::BeginSeeking,
        RemoteInput::HoldBegan(PlaybackRequest::SeekForward)
    )]
    #[case::end(MPSeekCommandEventType::EndSeeking, RemoteInput::HoldEnded)]
    fn only_the_start_of_a_hold_seeks(
        #[case] phase: MPSeekCommandEventType,
        #[case] input: RemoteInput,
    ) {
        assert_eq!(
            RemoteInput::from_phase(PlaybackRequest::SeekForward, phase),
            input
        );
    }

    #[rstest]
    #[case::inside(42.5, Ok(RemoteInput::Scrub(Duration::from_millis(42_500))))]
    #[case::negative(-1.0, Err(RemoteInputError::InvalidPosition))]
    #[case::not_a_number(f64::NAN, Err(RemoteInputError::InvalidPosition))]
    fn a_scrub_seeks_to_a_valid_position_only(
        #[case] seconds: f64,
        #[case] input: Result<RemoteInput, RemoteInputError>,
    ) {
        assert_eq!(RemoteInput::from_seconds(seconds), input);
    }

    #[derive(Debug, Clone, Copy)]
    enum QueueState {
        Room,
        Full,
        Closed,
    }

    #[rstest]
    #[case::room(QueueState::Room, MPRemoteCommandHandlerStatus::Success)]
    #[case::full(QueueState::Full, MPRemoteCommandHandlerStatus::CommandFailed)]
    #[case::closed(QueueState::Closed, MPRemoteCommandHandlerStatus::CommandFailed)]
    fn the_command_status_follows_the_send_result(
        #[case] queue_state: QueueState,
        #[case] expected: MPRemoteCommandHandlerStatus,
    ) {
        let (heard, receiver) = bounded(1);
        match queue_state {
            QueueState::Room => {}
            QueueState::Full => heard.try_send(MacosMessage::Started).unwrap(),
            QueueState::Closed => drop(receiver),
        }
        assert_eq!(status(heard.try_send(MacosMessage::Started)), expected);
    }
}
