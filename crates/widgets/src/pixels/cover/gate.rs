use std::path::{Path, PathBuf};

use kernel::{
    cmd::{Cmd, Cue},
    update::machine::{Machine, Unhandled},
};

use crate::pixels::cover::CrossfadePermit;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CrossfadeGate {
    #[default]
    None,
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

impl CrossfadeGate {
    pub fn permit(&mut self, cues: &[Cue], track: Option<&Path>) -> CrossfadePermit {
        if cues.contains(&Cue::TrackChanged) {
            self.settle(CrossfadeGateMessage::TrackChanged(
                track.map(Path::to_path_buf),
            ));
        }
        match self.transition(CrossfadeGateMessage::PermitTaken) {
            Ok(cmd) => cmd
                .effects()
                .copied()
                .last()
                .unwrap_or(CrossfadePermit::Withheld),
            Err(Unhandled) => CrossfadePermit::Withheld,
        }
    }

    pub fn cover_arrived(&mut self, path: &Path, arrival: CoverArrival) {
        self.settle(CrossfadeGateMessage::CoverArrived {
            path: path.to_path_buf(),
            arrival,
        });
    }

    fn settle(&mut self, message: CrossfadeGateMessage) {
        let effects = match self.transition(message) {
            Ok(cmd) => cmd.effects().count(),
            Err(Unhandled) => 0,
        };
        debug_assert_eq!(effects, 0);
    }
}

impl Machine for CrossfadeGate {
    type Message = CrossfadeGateMessage;
    type Effect = Cmd<CrossfadePermit, CrossfadeGateMessage>;

    fn transition(
        &mut self,
        message: CrossfadeGateMessage,
    ) -> Result<Self::Effect, Unhandled> {
        match (&*self, message) {
            (
                CrossfadeGate::None
                | CrossfadeGate::AwaitingCover(_)
                | CrossfadeGate::CoverReady(_),
                CrossfadeGateMessage::TrackChanged(track),
            ) => {
                *self = track.map_or(CrossfadeGate::None, CrossfadeGate::AwaitingCover);
                Ok(Cmd::none())
            }
            (
                CrossfadeGate::AwaitingCover(awaited),
                CrossfadeGateMessage::CoverArrived { path, arrival },
            ) if *awaited == path => {
                *self = match arrival {
                    CoverArrival::Decoded => CrossfadeGate::CoverReady(path),
                    CoverArrival::Missing => CrossfadeGate::None,
                };
                Ok(Cmd::none())
            }
            (CrossfadeGate::CoverReady(_), CrossfadeGateMessage::PermitTaken) => {
                *self = CrossfadeGate::None;
                Ok(Cmd::effect(CrossfadePermit::Allowed))
            }
            (
                CrossfadeGate::None
                | CrossfadeGate::AwaitingCover(_)
                | CrossfadeGate::CoverReady(_),
                CrossfadeGateMessage::CoverArrived { .. }
                | CrossfadeGateMessage::PermitTaken,
            ) => Err(Unhandled),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use kernel::{
        cmd::{Cmd, Cue},
        update::machine::{Machine, Unhandled},
    };
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
        CrossfadeGate::None,
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
        CrossfadeGate::None
    )]
    fn a_track_change_awaits_the_cover_of_the_current_track(
        #[case] before: CrossfadeGate,
        #[case] track: Option<&str>,
        #[case] after: CrossfadeGate,
    ) {
        let mut pending = before;

        let message = CrossfadeGateMessage::TrackChanged(track.map(PathBuf::from));

        assert_eq!(pending.transition(message), Ok(Cmd::none()));
        assert_eq!(pending, after);
    }

    #[rstest]
    #[case::a_decoded_cover_becomes_ready(
        arrived("/music/new.jpg", CoverArrival::Decoded),
        ready("/music/new.jpg")
    )]
    #[case::a_missing_cover_cancels_the_crossfade(
        arrived("/music/new.jpg", CoverArrival::Missing),
        CrossfadeGate::None
    )]
    fn the_awaited_cover_arriving_settles_the_crossfade(
        #[case] message: CrossfadeGateMessage,
        #[case] after: CrossfadeGate,
    ) {
        let mut pending = awaiting("/music/new.jpg");

        assert_eq!(pending.transition(message), Ok(Cmd::none()));
        assert_eq!(pending, after);
    }

    #[rstest]
    #[case::a_cover_for_a_different_track(
        awaiting("/music/new.jpg"),
        arrived("/music/other.jpg", CoverArrival::Decoded)
    )]
    #[case::a_cover_with_nothing_awaited(
        CrossfadeGate::None,
        arrived("/music/new.jpg", CoverArrival::Decoded)
    )]
    #[case::a_cover_already_ready(
        ready("/music/new.jpg"),
        arrived("/music/new.jpg", CoverArrival::Missing)
    )]
    #[case::a_permit_while_nothing_is_pending(
        CrossfadeGate::None,
        CrossfadeGateMessage::PermitTaken
    )]
    #[case::a_permit_while_the_cover_is_awaited(
        awaiting("/music/new.jpg"),
        CrossfadeGateMessage::PermitTaken
    )]
    fn an_unexpected_message_is_unhandled_and_keeps_the_state(
        #[case] before: CrossfadeGate,
        #[case] message: CrossfadeGateMessage,
    ) {
        let mut pending = before.clone();

        assert_eq!(pending.transition(message), Err(Unhandled));
        assert_eq!(pending, before);
    }

    #[test]
    fn a_ready_cover_allows_one_crossfade() {
        let mut pending = ready("/music/new.jpg");

        assert_eq!(
            pending.transition(CrossfadeGateMessage::PermitTaken),
            Ok(Cmd::effect(CrossfadePermit::Allowed))
        );
        assert_eq!(pending, CrossfadeGate::None);
    }

    #[test]
    fn a_track_change_cue_with_a_ready_cover_withholds_the_permit() {
        let mut gate = ready("/music/old.jpg");

        let permit =
            gate.permit(&[Cue::TrackChanged], Some(Path::new("/music/new.jpg")));

        assert_eq!(permit, CrossfadePermit::Withheld);
        assert_eq!(gate, awaiting("/music/new.jpg"));
    }

    #[test]
    fn the_cover_of_the_changed_track_arriving_allows_the_next_permit_once() {
        let mut gate = CrossfadeGate::None;
        let track = Path::new("/music/new.jpg");
        gate.permit(&[Cue::TrackChanged], Some(track));
        gate.cover_arrived(track, CoverArrival::Decoded);

        assert_eq!(gate.permit(&[], Some(track)), CrossfadePermit::Allowed);
        assert_eq!(gate.permit(&[], Some(track)), CrossfadePermit::Withheld);
    }
}
