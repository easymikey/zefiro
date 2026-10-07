#![forbid(unsafe_code)]

use std::time::Duration;

use kernel::message::PlaybackRequest;
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent,
    MPRemoteCommandEvent,
    MPSeekCommandEvent,
    MPSeekCommandEventType,
};

use crate::ffi::{self, Trigger};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RemoteInput {
    Press(PlaybackRequest),
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
            RemoteInput::Press(request)
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        domain::{direction::Direction, transport::SEEK_MEDIUM},
        message::PlaybackRequest,
    };
    use objc2_media_player::MPSeekCommandEventType;
    use rstest::rstest;

    use crate::remote_input::{RemoteInput, RemoteInputError};

    #[rstest]
    #[case::begin(
        MPSeekCommandEventType::BeginSeeking,
        RemoteInput::Press(PlaybackRequest::SeekBy { direction: Direction::Next, by: SEEK_MEDIUM })
    )]
    #[case::end(MPSeekCommandEventType::EndSeeking, RemoteInput::HoldEnded)]
    fn only_the_start_of_a_hold_seeks(
        #[case] phase: MPSeekCommandEventType,
        #[case] input: RemoteInput,
    ) {
        assert_eq!(
            RemoteInput::from_phase(
                PlaybackRequest::SeekBy {
                    direction: Direction::Next,
                    by: SEEK_MEDIUM
                },
                phase
            ),
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
}
