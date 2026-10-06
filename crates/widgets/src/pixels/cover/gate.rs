use std::path::PathBuf;

use kernel::update::machine::{Machine, Unhandled};

use crate::pixels::cover::CrossfadePermit;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CrossfadeGate {
    #[default]
    Idle,
    AwaitingCover(PathBuf),
    CoverReady(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverArrival {
    Decoded,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrossfadeGateMessage {
    TrackChanged(Option<PathBuf>),
    CoverArrived {
        path: PathBuf,
        arrival: CoverArrival,
    },
    PermitTaken,
}

impl Machine for CrossfadeGate {
    type Message = CrossfadeGateMessage;
    type Effect = CrossfadePermit;

    fn transition(
        &mut self,
        message: CrossfadeGateMessage,
    ) -> Result<Self::Effect, Unhandled> {
        match (&*self, message) {
            (
                CrossfadeGate::Idle
                | CrossfadeGate::AwaitingCover(_)
                | CrossfadeGate::CoverReady(_),
                CrossfadeGateMessage::TrackChanged(track),
            ) => {
                *self = track.map_or(CrossfadeGate::Idle, CrossfadeGate::AwaitingCover);
                Ok(CrossfadePermit::Withheld)
            }
            (
                CrossfadeGate::AwaitingCover(awaited),
                CrossfadeGateMessage::CoverArrived { path, arrival },
            ) if *awaited == path => {
                *self = match arrival {
                    CoverArrival::Decoded => CrossfadeGate::CoverReady(path),
                    CoverArrival::Missing => CrossfadeGate::Idle,
                };
                Ok(CrossfadePermit::Withheld)
            }
            (CrossfadeGate::CoverReady(_), CrossfadeGateMessage::PermitTaken) => {
                *self = CrossfadeGate::Idle;
                Ok(CrossfadePermit::Allowed)
            }
            (
                CrossfadeGate::Idle | CrossfadeGate::AwaitingCover(_),
                CrossfadeGateMessage::PermitTaken,
            ) => Ok(CrossfadePermit::Withheld),
            (
                CrossfadeGate::Idle
                | CrossfadeGate::AwaitingCover(_)
                | CrossfadeGate::CoverReady(_),
                CrossfadeGateMessage::CoverArrived { .. },
            ) => Err(Unhandled),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::update::machine::{Machine, Unhandled};
    use rstest::rstest;

    use crate::pixels::cover::{
        CrossfadePermit,
        gate::{CoverArrival, CrossfadeGate, CrossfadeGateMessage},
    };

    fn awaiting(path: &str) -> CrossfadeGate {
        CrossfadeGate::AwaitingCover(PathBuf::from(path))
    }

    fn ready(path: &str) -> CrossfadeGate {
        CrossfadeGate::CoverReady(PathBuf::from(path))
    }

    fn arrived(path: &str, arrival: CoverArrival) -> CrossfadeGateMessage {
        CrossfadeGateMessage::CoverArrived {
            path: PathBuf::from(path),
            arrival,
        }
    }

    #[rstest]
    #[case::a_current_track_awaits_its_cover(
        CrossfadeGate::Idle,
        Some("/music/new.jpg"),
        awaiting("/music/new.jpg")
    )]
    #[case::a_ready_cover_of_the_old_track_is_dropped(
        ready("/music/old.jpg"),
        Some("/music/new.jpg"),
        awaiting("/music/new.jpg")
    )]
    #[case::no_current_track_awaits_nothing(
        awaiting("/music/old.jpg"),
        None,
        CrossfadeGate::Idle
    )]
    #[case::another_track_replaces_the_awaited_cover(
        awaiting("/music/old.jpg"),
        Some("/music/other.jpg"),
        awaiting("/music/other.jpg")
    )]
    #[case::no_current_track_drops_a_ready_cover(
        ready("/music/old.jpg"),
        None,
        CrossfadeGate::Idle
    )]
    #[case::no_current_track_with_nothing_pending(
        CrossfadeGate::Idle,
        None,
        CrossfadeGate::Idle
    )]
    fn a_track_change_awaits_the_cover_of_the_current_track(
        #[case] before: CrossfadeGate,
        #[case] track_path: Option<&str>,
        #[case] after: CrossfadeGate,
    ) {
        let mut pending = before;

        let message = CrossfadeGateMessage::TrackChanged(track_path.map(PathBuf::from));

        assert_eq!(pending.transition(message), Ok(CrossfadePermit::Withheld));
        assert_eq!(pending, after);
    }

    #[rstest]
    #[case::a_decoded_cover_becomes_ready(
        arrived("/music/new.jpg", CoverArrival::Decoded),
        ready("/music/new.jpg")
    )]
    #[case::a_missing_cover_cancels_the_crossfade(
        arrived("/music/new.jpg", CoverArrival::Missing),
        CrossfadeGate::Idle
    )]
    fn the_awaited_cover_arriving_settles_the_crossfade(
        #[case] message: CrossfadeGateMessage,
        #[case] after: CrossfadeGate,
    ) {
        let mut pending = awaiting("/music/new.jpg");

        assert_eq!(pending.transition(message), Ok(CrossfadePermit::Withheld));
        assert_eq!(pending, after);
    }

    #[rstest]
    #[case::a_cover_for_a_different_track(
        awaiting("/music/new.jpg"),
        arrived("/music/other.jpg", CoverArrival::Decoded)
    )]
    #[case::a_cover_with_nothing_awaited(
        CrossfadeGate::Idle,
        arrived("/music/new.jpg", CoverArrival::Decoded)
    )]
    #[case::a_cover_already_ready(
        ready("/music/new.jpg"),
        arrived("/music/new.jpg", CoverArrival::Missing)
    )]
    fn an_unexpected_message_is_unhandled_and_keeps_the_state(
        #[case] before: CrossfadeGate,
        #[case] message: CrossfadeGateMessage,
    ) {
        let mut pending = before.clone();

        assert_eq!(pending.transition(message), Err(Unhandled));
        assert_eq!(pending, before);
    }

    #[rstest]
    #[case::a_permit_while_nothing_is_pending(CrossfadeGate::Idle)]
    #[case::a_permit_while_the_cover_is_awaited(awaiting("/music/new.jpg"))]
    fn a_permit_without_a_ready_cover_is_withheld_and_keeps_the_state(
        #[case] before: CrossfadeGate,
    ) {
        let mut pending = before.clone();

        assert_eq!(
            pending.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(pending, before);
    }

    #[test]
    fn a_ready_cover_allows_one_crossfade() {
        let mut pending = ready("/music/new.jpg");

        assert_eq!(
            pending.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Allowed)
        );
        assert_eq!(pending, CrossfadeGate::Idle);
    }

    fn changed(path: &str) -> CrossfadeGateMessage {
        CrossfadeGateMessage::TrackChanged(Some(PathBuf::from(path)))
    }

    #[test]
    fn a_track_change_with_a_ready_cover_withholds_the_permit() {
        let mut gate = ready("/music/old.jpg");

        assert_eq!(
            gate.transition(changed("/music/new.jpg")),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(
            gate.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(gate, awaiting("/music/new.jpg"));
    }

    #[test]
    fn a_cover_arriving_in_the_same_frame_as_the_track_change_allows_the_permit() {
        let mut gate = ready("/music/old.jpg");

        assert_eq!(
            gate.transition(changed("/music/new.jpg")),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(
            gate.transition(arrived("/music/new.jpg", CoverArrival::Decoded)),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(
            gate.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Allowed)
        );
    }

    #[test]
    fn the_cover_of_the_changed_track_arriving_allows_the_next_permit_once() {
        let mut gate = CrossfadeGate::Idle;

        assert_eq!(
            gate.transition(changed("/music/new.jpg")),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(
            gate.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(
            gate.transition(arrived("/music/new.jpg", CoverArrival::Decoded)),
            Ok(CrossfadePermit::Withheld)
        );
        assert_eq!(
            gate.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Allowed)
        );
        assert_eq!(
            gate.transition(CrossfadeGateMessage::PermitTaken),
            Ok(CrossfadePermit::Withheld)
        );
    }
}
