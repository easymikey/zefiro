#![forbid(unsafe_code)]

use std::{ptr::NonNull, time::Duration};

use block2::RcBlock;
use crossbeam_channel::{Sender, TrySendError};
use kernel::{MacosEvent, Message, PlaybackRequest};
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent,
    MPRemoteCommand,
    MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
    MPSeekCommandEvent,
    MPSeekCommandEventType,
};

use crate::ffi;

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
        events: &Sender<Message>,
    ) -> Self {
        let center = ffi::shared_command_center();
        let commands = ffi::remote_commands(&center);
        let targets = commands
            .into_iter()
            .map(|(command, trigger)| {
                let handler = RcBlock::new(command_handler(trigger, events.clone()));
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
    events: Sender<Message>,
) -> impl Fn(NonNull<MPRemoteCommandEvent>) -> MPRemoteCommandHandlerStatus + 'static {
    move |event| {
        ffi::borrow_command_event(event, |event| match outcome(trigger, event) {
            CommandOutcome::Forward(request) => {
                status(events.try_send(Message::from(MacosEvent::MediaKey(request))))
            }
            CommandOutcome::Nothing => MPRemoteCommandHandlerStatus::Success,
            CommandOutcome::Malformed => MPRemoteCommandHandlerStatus::CommandFailed,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CommandOutcome {
    Forward(PlaybackRequest),
    Nothing,
    Malformed,
}

fn outcome(trigger: Trigger, event: &MPRemoteCommandEvent) -> CommandOutcome {
    match trigger {
        Trigger::Press(request) => CommandOutcome::Forward(request),
        Trigger::Hold(request) => event
            .downcast_ref::<MPSeekCommandEvent>()
            .map_or(CommandOutcome::Malformed, |seek| {
                hold(request, ffi::seek_event_phase(seek))
            }),
        Trigger::Scrub => event
            .downcast_ref::<MPChangePlaybackPositionCommandEvent>()
            .map_or(CommandOutcome::Malformed, |scrub| {
                scrub_to(ffi::scrub_position_seconds(scrub))
            }),
    }
}

fn hold(request: PlaybackRequest, phase: MPSeekCommandEventType) -> CommandOutcome {
    if phase == MPSeekCommandEventType::BeginSeeking {
        CommandOutcome::Forward(request)
    } else {
        CommandOutcome::Nothing
    }
}

fn scrub_to(seconds: f64) -> CommandOutcome {
    Duration::try_from_secs_f64(seconds).map_or(CommandOutcome::Malformed, |position| {
        CommandOutcome::Forward(PlaybackRequest::SeekTo(position))
    })
}

fn status(sent: Result<(), TrySendError<Message>>) -> MPRemoteCommandHandlerStatus {
    sent.map_or(MPRemoteCommandHandlerStatus::CommandFailed, |()| {
        MPRemoteCommandHandlerStatus::Success
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::bounded;
    use kernel::{Message, PlaybackRequest};
    use objc2_media_player::{MPRemoteCommandHandlerStatus, MPSeekCommandEventType};
    use rstest::rstest;

    use crate::controls::{CommandOutcome, hold, scrub_to, status};

    #[rstest]
    #[case::begin(
        MPSeekCommandEventType::BeginSeeking,
        CommandOutcome::Forward(PlaybackRequest::SeekForward)
    )]
    #[case::end(MPSeekCommandEventType::EndSeeking, CommandOutcome::Nothing)]
    fn only_the_start_of_a_hold_seeks(
        #[case] phase: MPSeekCommandEventType,
        #[case] outcome: CommandOutcome,
    ) {
        assert_eq!(hold(PlaybackRequest::SeekForward, phase), outcome);
    }

    #[rstest]
    #[case::inside(
        42.5,
        CommandOutcome::Forward(PlaybackRequest::SeekTo(Duration::from_millis(
            42_500
        )))
    )]
    #[case::negative(-1.0, CommandOutcome::Malformed)]
    #[case::not_a_number(f64::NAN, CommandOutcome::Malformed)]
    fn a_scrub_seeks_to_a_valid_position_only(
        #[case] seconds: f64,
        #[case] outcome: CommandOutcome,
    ) {
        assert_eq!(scrub_to(seconds), outcome);
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
        let (events, receiver) = bounded(1);
        match queue_state {
            QueueState::Room => {}
            QueueState::Full => events.try_send(Message::Quit).unwrap(),
            QueueState::Closed => drop(receiver),
        }
        assert_eq!(status(events.try_send(Message::Quit)), expected);
    }
}
