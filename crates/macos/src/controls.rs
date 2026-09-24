#![forbid(unsafe_code)]

use std::{ptr::NonNull, time::Duration};

use block2::RcBlock;
use crossbeam_channel::{SendError, Sender};
use kernel::{Message, PlaybackRequest};
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
enum Gesture {
    Press(PlaybackRequest),
    Hold(PlaybackRequest),
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
            .zip(gestures())
            .map(|(command, gesture)| {
                let target = target(&command, gesture, events.clone());
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

fn gestures() -> [Gesture; 9] {
    [
        Gesture::Press(PlaybackRequest::Play),
        Gesture::Press(PlaybackRequest::Pause),
        Gesture::Press(PlaybackRequest::Toggle),
        Gesture::Press(PlaybackRequest::Stop),
        Gesture::Press(PlaybackRequest::Next),
        Gesture::Press(PlaybackRequest::Prev),
        Gesture::Hold(PlaybackRequest::SeekForward),
        Gesture::Hold(PlaybackRequest::SeekBack),
        Gesture::Scrub,
    ]
}

fn target(
    command: &MPRemoteCommand,
    gesture: Gesture,
    events: Sender<Message>,
) -> Retained<AnyObject> {
    let handler = RcBlock::new(handler(gesture, events));
    ffi::enable_command(command);
    ffi::add_command_target(command, &handler)
}

fn handler(
    gesture: Gesture,
    events: Sender<Message>,
) -> impl Fn(NonNull<MPRemoteCommandEvent>) -> MPRemoteCommandHandlerStatus + 'static {
    move |event| {
        ffi::borrow_command_event(event, |event| match reaction(gesture, event) {
            Reaction::Forward(message) => status(events.send(message)),
            Reaction::Nothing => MPRemoteCommandHandlerStatus::Success,
            Reaction::Malformed => MPRemoteCommandHandlerStatus::CommandFailed,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Reaction {
    Forward(Message),
    Nothing,
    Malformed,
}

fn reaction(gesture: Gesture, event: &MPRemoteCommandEvent) -> Reaction {
    match gesture {
        Gesture::Press(request) => Reaction::Forward(Message::Playback(request)),
        Gesture::Hold(request) => event
            .downcast_ref::<MPSeekCommandEvent>()
            .map_or(Reaction::Malformed, |seek| hold(request, phase(seek))),
        Gesture::Scrub => event
            .downcast_ref::<MPChangePlaybackPositionCommandEvent>()
            .map_or(Reaction::Malformed, |scrub| scrub_to(seconds(scrub))),
    }
}

fn phase(seek: &MPSeekCommandEvent) -> MPSeekCommandEventType {
    ffi::seek_event_phase(seek)
}

fn seconds(scrub: &MPChangePlaybackPositionCommandEvent) -> f64 {
    ffi::scrub_position_seconds(scrub)
}

fn hold(request: PlaybackRequest, phase: MPSeekCommandEventType) -> Reaction {
    if phase == MPSeekCommandEventType::BeginSeeking {
        Reaction::Forward(Message::Playback(request))
    } else {
        Reaction::Nothing
    }
}

fn scrub_to(seconds: f64) -> Reaction {
    Duration::try_from_secs_f64(seconds).map_or(Reaction::Malformed, |position| {
        Reaction::Forward(Message::Playback(PlaybackRequest::SeekTo(position)))
    })
}

fn status(sent: Result<(), SendError<Message>>) -> MPRemoteCommandHandlerStatus {
    sent.map_or(MPRemoteCommandHandlerStatus::CommandFailed, |()| {
        MPRemoteCommandHandlerStatus::Success
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{Message, PlaybackRequest};
    use objc2_media_player::{MPRemoteCommandHandlerStatus, MPSeekCommandEventType};
    use rstest::rstest;

    use crate::controls::{Reaction, hold, scrub_to, status};

    #[rstest]
    #[case::begin(
        MPSeekCommandEventType::BeginSeeking,
        Reaction::Forward(Message::Playback(PlaybackRequest::SeekForward))
    )]
    #[case::end(MPSeekCommandEventType::EndSeeking, Reaction::Nothing)]
    fn only_the_start_of_a_hold_seeks(
        #[case] phase: MPSeekCommandEventType,
        #[case] reaction: Reaction,
    ) {
        assert_eq!(hold(PlaybackRequest::SeekForward, phase), reaction);
    }

    #[rstest]
    #[case::inside(
        42.5,
        Reaction::Forward(Message::Playback(PlaybackRequest::SeekTo(
            Duration::from_millis(42_500)
        )))
    )]
    #[case::negative(-1.0, Reaction::Malformed)]
    #[case::not_a_number(f64::NAN, Reaction::Malformed)]
    fn a_scrub_seeks_to_a_valid_position_only(
        #[case] seconds: f64,
        #[case] reaction: Reaction,
    ) {
        assert_eq!(scrub_to(seconds), reaction);
    }

    #[test]
    fn a_closed_event_channel_fails_the_command() {
        let (events, receiver) = crossbeam_channel::unbounded::<Message>();
        assert_eq!(
            status(events.send(Message::Quit)),
            MPRemoteCommandHandlerStatus::Success
        );
        drop(receiver);
        assert_eq!(
            status(events.send(Message::Quit)),
            MPRemoteCommandHandlerStatus::CommandFailed
        );
    }
}
