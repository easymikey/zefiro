use std::path::{Path, PathBuf};

use kernel::{
    Cmd,
    domain::{appearance::CoverMode, geometry::Cells},
    update::{Machine, Unhandled},
};
use widgets::{CoverWash, CrossfadePermit};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum CrossfadeGate {
    #[default]
    None,
    AwaitingCover(PathBuf),
    CoverReady(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverArrival {
    Decoded,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CrossfadeGateMessage {
    TrackChanged(Option<PathBuf>),
    CoverArrived {
        path: PathBuf,
        arrival: CoverArrival,
    },
    PermitTaken,
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

#[must_use]
pub(crate) fn cover_wash(progress: Option<f32>, screen_width: Cells) -> CoverWash {
    progress.map_or(CoverWash::Idle, |progress| CoverWash::Running {
        progress,
        screen_width,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverWant {
    None,
    Same,
    New(PathBuf),
}

pub(crate) fn wanted_cover(
    wanted: Option<&Path>,
    current_track: Option<&Path>,
    mode: CoverMode,
) -> CoverWant {
    let current_track =
        current_track.filter(|_| matches!(mode, CoverMode::Plain | CoverMode::Vinyl));
    match current_track {
        None => CoverWant::None,
        Some(path) if wanted == Some(path) => CoverWant::Same,
        Some(path) => CoverWant::New(path.to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use kernel::{
        Cmd,
        domain::{appearance::CoverMode, geometry::Cells},
        update::{Machine, Unhandled},
    };
    use rstest::rstest;
    use widgets::{CoverWash, CrossfadePermit};

    use crate::shell::cover_crossfade::{
        CoverArrival,
        CoverWant,
        CrossfadeGate,
        CrossfadeGateMessage,
        cover_wash,
        wanted_cover,
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
    #[case::an_off_style_wants_no_cover(CoverMode::Off, CoverWant::None)]
    #[case::a_milkdrop_style_wants_no_decoded_cover(
        CoverMode::Milkdrop,
        CoverWant::None
    )]
    #[case::a_plain_style_wants_the_track_cover(
        CoverMode::Plain,
        CoverWant::New(PathBuf::from("/music/track.jpg"))
    )]
    #[case::a_vinyl_style_wants_the_track_cover(
        CoverMode::Vinyl,
        CoverWant::New(PathBuf::from("/music/track.jpg"))
    )]
    fn the_cover_style_decides_whether_a_cover_is_wanted(
        #[case] mode: CoverMode,
        #[case] expected: CoverWant,
    ) {
        let want = wanted_cover(None, Some(Path::new("/music/track.jpg")), mode);

        assert_eq!(want, expected);
    }

    #[rstest]
    #[case::playback_stopping_wants_no_cover(
        Some("/music/old.jpg"),
        None,
        CoverWant::None
    )]
    #[case::a_new_track_path_is_wanted(
        None,
        Some("/music/track.jpg"),
        CoverWant::New(PathBuf::from("/music/track.jpg"))
    )]
    #[case::the_same_track_path_is_wanted_already(
        Some("/music/track.jpg"),
        Some("/music/track.jpg"),
        CoverWant::Same
    )]
    #[case::a_changed_track_path_is_wanted_anew(
        Some("/music/old.jpg"),
        Some("/music/new.jpg"),
        CoverWant::New(PathBuf::from("/music/new.jpg"))
    )]
    fn the_wanted_cover_follows_the_current_track(
        #[case] wanted: Option<&str>,
        #[case] current: Option<&str>,
        #[case] expected: CoverWant,
    ) {
        let want = wanted_cover(
            wanted.map(Path::new),
            current.map(Path::new),
            CoverMode::Plain,
        );

        assert_eq!(want, expected);
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
    fn no_wash_progress_is_an_idle_wash() {
        assert_eq!(cover_wash(None, Cells(80)), CoverWash::Idle);
    }

    #[test]
    fn a_wash_progress_carries_the_screen_width_along() {
        assert_eq!(
            cover_wash(Some(0.4), Cells(80)),
            CoverWash::Running {
                progress: 0.4,
                screen_width: Cells(80),
            }
        );
    }
}
