#![forbid(unsafe_code)]

use std::{ptr::NonNull, time::Duration};

use block2::RcBlock;
use crossbeam_channel::{Sender, TrySendError};
use kernel::{Gesture, MacosEvent, Message};
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
enum Trigger {
    Press(Gesture),
    Hold(Gesture),
    Scrub,
}

#[derive(Debug)]
pub struct Controls {
    targets: Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>,
}

impl Controls {
    #[must_use]
    pub fn attach(_main_thread: MainThreadMarker, events: &Sender<Message>) -> Self {
        let center = ffi::shared_command_center();
        let commands = ffi::remote_commands(&center);
        let targets = commands
            .into_iter()
            .zip(triggers())
            .map(|(command, trigger)| {
                let target = target(&command, trigger, events.clone());
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

fn triggers() -> [Trigger; 9] {
    [
        Trigger::Press(Gesture::Play),
        Trigger::Press(Gesture::Pause),
        Trigger::Press(Gesture::Toggle),
        Trigger::Press(Gesture::Stop),
        Trigger::Press(Gesture::Next),
        Trigger::Press(Gesture::Previous),
        Trigger::Hold(Gesture::SeekForward),
        Trigger::Hold(Gesture::SeekBack),
        Trigger::Scrub,
    ]
}

#[cfg(test)]
fn pressed(trigger: Trigger) -> Option<Gesture> {
    match trigger {
        Trigger::Press(gesture) => Some(gesture),
        Trigger::Hold(_) | Trigger::Scrub => None,
    }
}

fn target(
    command: &MPRemoteCommand,
    trigger: Trigger,
    events: Sender<Message>,
) -> Retained<AnyObject> {
    let handler = RcBlock::new(command_handler(trigger, events));
    ffi::enable_command(command);
    ffi::add_command_target(command, &handler)
}

fn command_handler(
    trigger: Trigger,
    events: Sender<Message>,
) -> impl Fn(NonNull<MPRemoteCommandEvent>) -> MPRemoteCommandHandlerStatus + 'static {
    move |event| {
        ffi::borrow_command_event(event, |event| match reaction(trigger, event) {
            CommandOutcome::Forward(gesture) => {
                status(events.try_send(Message::from(MacosEvent::MediaKey(gesture))))
            }
            CommandOutcome::Nothing => MPRemoteCommandHandlerStatus::Success,
            CommandOutcome::Malformed => MPRemoteCommandHandlerStatus::CommandFailed,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandOutcome {
    Forward(Gesture),
    Nothing,
    Malformed,
}

fn reaction(trigger: Trigger, event: &MPRemoteCommandEvent) -> CommandOutcome {
    match trigger {
        Trigger::Press(gesture) => CommandOutcome::Forward(gesture),
        Trigger::Hold(gesture) => event
            .downcast_ref::<MPSeekCommandEvent>()
            .map_or(CommandOutcome::Malformed, |seek| hold(gesture, phase(seek))),
        Trigger::Scrub => event
            .downcast_ref::<MPChangePlaybackPositionCommandEvent>()
            .map_or(CommandOutcome::Malformed, |scrub| scrub_to(seconds(scrub))),
    }
}

fn phase(seek: &MPSeekCommandEvent) -> MPSeekCommandEventType {
    ffi::seek_event_phase(seek)
}

fn seconds(scrub: &MPChangePlaybackPositionCommandEvent) -> f64 {
    ffi::scrub_position_seconds(scrub)
}

fn hold(gesture: Gesture, phase: MPSeekCommandEventType) -> CommandOutcome {
    if phase == MPSeekCommandEventType::BeginSeeking {
        CommandOutcome::Forward(gesture)
    } else {
        CommandOutcome::Nothing
    }
}

fn scrub_to(seconds: f64) -> CommandOutcome {
    Duration::try_from_secs_f64(seconds).map_or(CommandOutcome::Malformed, |position| {
        CommandOutcome::Forward(Gesture::Scrub(position))
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
    use kernel::{Gesture, Message};
    use objc2_media_player::{MPRemoteCommandHandlerStatus, MPSeekCommandEventType};
    use rstest::rstest;

    use crate::controls::{CommandOutcome, hold, pressed, scrub_to, status, triggers};

    #[rstest]
    #[case::begin(
        MPSeekCommandEventType::BeginSeeking,
        CommandOutcome::Forward(Gesture::SeekForward)
    )]
    #[case::end(MPSeekCommandEventType::EndSeeking, CommandOutcome::Nothing)]
    fn only_the_start_of_a_hold_seeks(
        #[case] phase: MPSeekCommandEventType,
        #[case] reaction: CommandOutcome,
    ) {
        assert_eq!(hold(Gesture::SeekForward, phase), reaction);
    }

    #[rstest]
    #[case::inside(
        42.5,
        CommandOutcome::Forward(Gesture::Scrub(Duration::from_millis(42_500)))
    )]
    #[case::negative(-1.0, CommandOutcome::Malformed)]
    #[case::not_a_number(f64::NAN, CommandOutcome::Malformed)]
    fn a_scrub_seeks_to_a_valid_position_only(
        #[case] seconds: f64,
        #[case] reaction: CommandOutcome,
    ) {
        assert_eq!(scrub_to(seconds), reaction);
    }

    #[derive(Debug, Clone, Copy)]
    enum Mailbox {
        Room,
        Full,
        Closed,
    }

    #[rstest]
    #[case::room(Mailbox::Room, MPRemoteCommandHandlerStatus::Success)]
    #[case::full(Mailbox::Full, MPRemoteCommandHandlerStatus::CommandFailed)]
    #[case::closed(Mailbox::Closed, MPRemoteCommandHandlerStatus::CommandFailed)]
    fn the_command_status_follows_the_mailbox(
        #[case] mailbox: Mailbox,
        #[case] expected: MPRemoteCommandHandlerStatus,
    ) {
        let (events, receiver) = bounded(1);
        match mailbox {
            Mailbox::Room => {}
            Mailbox::Full => events.try_send(Message::Quit).unwrap(),
            Mailbox::Closed => drop(receiver),
        }
        assert_eq!(status(events.try_send(Message::Quit)), expected);
    }

    #[test]
    fn every_trigger_forwards_its_gesture() {
        let presses: Vec<Option<Gesture>> =
            triggers().into_iter().map(pressed).collect();
        assert_eq!(
            presses,
            vec![
                Some(Gesture::Play),
                Some(Gesture::Pause),
                Some(Gesture::Toggle),
                Some(Gesture::Stop),
                Some(Gesture::Next),
                Some(Gesture::Previous),
                None,
                None,
                None,
            ]
        );
    }
}
