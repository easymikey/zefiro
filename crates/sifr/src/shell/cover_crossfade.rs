use std::path::{Path, PathBuf};

use config::CoverStyle;
use kernel::Cue;
use terminal::{CoverWash, CrossfadePermit};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum PendingCrossfade {
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

#[must_use]
pub(crate) fn cover_wash(progress: Option<f32>, screen_width: u16) -> CoverWash {
    progress.map_or(CoverWash::Idle, |progress| CoverWash::Running {
        progress,
        screen_width,
    })
}

pub(crate) fn after_track_change(
    current: PendingCrossfade,
    cues: &[Cue],
    current_track: Option<&Path>,
) -> PendingCrossfade {
    if !cues.contains(&Cue::TrackChanged) {
        return current;
    }
    current_track.map_or(PendingCrossfade::None, |path| {
        PendingCrossfade::AwaitingCover(path.to_path_buf())
    })
}

pub(crate) fn after_cover_arrival(
    current: PendingCrossfade,
    path: &Path,
    arrival: CoverArrival,
) -> PendingCrossfade {
    let PendingCrossfade::AwaitingCover(awaited) = &current else {
        return current;
    };
    if awaited != path {
        return current;
    }
    match arrival {
        CoverArrival::Decoded => PendingCrossfade::CoverReady(path.to_path_buf()),
        CoverArrival::Missing => PendingCrossfade::None,
    }
}

pub(crate) fn take_crossfade_permit(
    current: PendingCrossfade,
) -> (PendingCrossfade, CrossfadePermit) {
    match current {
        PendingCrossfade::CoverReady(_) => {
            (PendingCrossfade::None, CrossfadePermit::Allowed)
        }
        other @ (PendingCrossfade::None | PendingCrossfade::AwaitingCover(_)) => {
            (other, CrossfadePermit::Withheld)
        }
    }
}

pub(crate) fn wanted_cover(
    wanted: &mut Option<PathBuf>,
    current_track: Option<&Path>,
    style: CoverStyle,
) -> Option<PathBuf> {
    let current_track = current_track
        .filter(|_| matches!(style, CoverStyle::Plain | CoverStyle::Vinyl));
    let Some(path) = current_track else {
        *wanted = None;
        return None;
    };
    if wanted.as_deref() == Some(path) {
        return None;
    }
    *wanted = Some(path.to_path_buf());
    Some(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use config::CoverStyle;
    use kernel::Cue;
    use terminal::{CoverWash, CrossfadePermit};

    use crate::shell::cover_crossfade::{
        CoverArrival,
        PendingCrossfade,
        after_cover_arrival,
        after_track_change,
        cover_wash,
        take_crossfade_permit,
        wanted_cover,
    };

    fn want(
        wanted: &mut Option<PathBuf>,
        current: Option<&str>,
        style: CoverStyle,
    ) -> Option<PathBuf> {
        wanted_cover(wanted, current.map(Path::new), style)
    }

    #[test]
    fn an_off_style_wants_no_cover_and_forgets_it() {
        let mut wanted = Some(PathBuf::from("/music/old.jpg"));

        let request = want(&mut wanted, Some("/music/old.jpg"), CoverStyle::Off);

        assert_eq!(request, None);
        assert_eq!(wanted, None);
    }

    #[test]
    fn a_milkdrop_style_wants_no_decoded_cover() {
        let mut wanted = None;

        let request = want(&mut wanted, Some("/music/track.jpg"), CoverStyle::Milkdrop);

        assert_eq!(request, None);
    }

    #[test]
    fn playback_stopping_forgets_the_wanted_cover() {
        let mut wanted = Some(PathBuf::from("/music/old.jpg"));

        let request = want(&mut wanted, None, CoverStyle::Plain);

        assert_eq!(request, None);
        assert_eq!(wanted, None);
    }

    #[test]
    fn a_new_track_path_is_requested() {
        let mut wanted = None;

        let request = want(&mut wanted, Some("/music/track.jpg"), CoverStyle::Vinyl);

        assert_eq!(request, Some(PathBuf::from("/music/track.jpg")));
        assert_eq!(wanted, Some(PathBuf::from("/music/track.jpg")));
    }

    #[test]
    fn the_same_track_path_is_not_requested_again() {
        let mut wanted = Some(PathBuf::from("/music/track.jpg"));

        let request = want(&mut wanted, Some("/music/track.jpg"), CoverStyle::Plain);

        assert_eq!(request, None);
    }

    #[test]
    fn a_changed_track_path_is_requested_again() {
        let mut wanted = Some(PathBuf::from("/music/old.jpg"));

        let request = want(&mut wanted, Some("/music/new.jpg"), CoverStyle::Plain);

        assert_eq!(request, Some(PathBuf::from("/music/new.jpg")));
        assert_eq!(wanted, Some(PathBuf::from("/music/new.jpg")));
    }

    #[test]
    fn a_track_change_with_a_current_track_awaits_its_cover() {
        let pending = after_track_change(
            PendingCrossfade::None,
            &[Cue::TrackChanged],
            Some(Path::new("/music/new.jpg")),
        );

        assert_eq!(
            pending,
            PendingCrossfade::AwaitingCover(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_resize_with_no_track_change_leaves_the_pending_crossfade_untouched() {
        let pending = after_track_change(
            PendingCrossfade::None,
            &[],
            Some(Path::new("/music/old.jpg")),
        );

        assert_eq!(pending, PendingCrossfade::None);
        assert_eq!(take_crossfade_permit(pending).1, CrossfadePermit::Withheld);
    }

    #[test]
    fn a_decoded_cover_for_the_awaited_track_becomes_ready() {
        let pending = after_cover_arrival(
            PendingCrossfade::AwaitingCover(PathBuf::from("/music/new.jpg")),
            Path::new("/music/new.jpg"),
            CoverArrival::Decoded,
        );

        assert_eq!(
            pending,
            PendingCrossfade::CoverReady(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_decoded_cover_for_a_different_track_is_ignored() {
        let pending = after_cover_arrival(
            PendingCrossfade::AwaitingCover(PathBuf::from("/music/new.jpg")),
            Path::new("/music/other.jpg"),
            CoverArrival::Decoded,
        );

        assert_eq!(
            pending,
            PendingCrossfade::AwaitingCover(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_missing_cover_for_the_awaited_track_cancels_the_pending_crossfade() {
        let pending = after_cover_arrival(
            PendingCrossfade::AwaitingCover(PathBuf::from("/music/new.jpg")),
            Path::new("/music/new.jpg"),
            CoverArrival::Missing,
        );

        assert_eq!(pending, PendingCrossfade::None);
    }

    #[test]
    fn a_ready_cover_allows_one_crossfade() {
        let (next, permit) = take_crossfade_permit(PendingCrossfade::CoverReady(
            PathBuf::from("/music/new.jpg"),
        ));

        assert_eq!(permit, CrossfadePermit::Allowed);
        assert_eq!(next, PendingCrossfade::None);
    }

    #[test]
    fn an_awaited_cover_withholds_the_crossfade_and_keeps_waiting() {
        let (next, permit) = take_crossfade_permit(PendingCrossfade::AwaitingCover(
            PathBuf::from("/music/new.jpg"),
        ));

        assert_eq!(permit, CrossfadePermit::Withheld);
        assert_eq!(
            next,
            PendingCrossfade::AwaitingCover(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_track_changes_then_its_cover_arrives_two_paints_later() {
        let pending = after_track_change(
            PendingCrossfade::None,
            &[Cue::TrackChanged],
            Some(Path::new("/music/new.jpg")),
        );
        let (pending, first_paint) = take_crossfade_permit(pending);
        assert_eq!(first_paint, CrossfadePermit::Withheld);

        let pending =
            after_track_change(pending, &[], Some(Path::new("/music/new.jpg")));
        let (pending, second_paint) = take_crossfade_permit(pending);
        assert_eq!(second_paint, CrossfadePermit::Withheld);

        let pending = after_cover_arrival(
            pending,
            Path::new("/music/new.jpg"),
            CoverArrival::Decoded,
        );
        let (pending, third_paint) = take_crossfade_permit(pending);
        assert_eq!(third_paint, CrossfadePermit::Allowed);

        let (_, fourth_paint) = take_crossfade_permit(pending);
        assert_eq!(fourth_paint, CrossfadePermit::Withheld);
    }

    #[test]
    fn no_wash_progress_is_an_idle_wash() {
        assert_eq!(cover_wash(None, 80), CoverWash::Idle);
    }

    #[test]
    fn a_wash_progress_carries_the_screen_width_along() {
        assert_eq!(
            cover_wash(Some(0.4), 80),
            CoverWash::Running {
                progress: 0.4,
                screen_width: 80,
            }
        );
    }
}
