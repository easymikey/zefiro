use std::path::{Path, PathBuf};

use config::CoverStyle;
use kernel::Cue;
use runtime::{CoverDecoded, CoverOutcome, CoverRequest};
use terminal::{CoverFade, CoverSources, CoverWash, DecodedCover};
use widgets::{FrameLayout, Scene};

use crate::toast::ShellFailure;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum CoverFadePermission {
    #[default]
    Absent,
    Awaiting(PathBuf),
    Ready(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverArrival {
    Decoded,
    Missing,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CoverPlacement {
    pub(crate) layout: FrameLayout,
    pub(crate) fade: CoverFade,
    pub(crate) wash: CoverWash,
}

pub(crate) fn cover_sources(
    scene: Scene<'_>,
    placement: CoverPlacement,
) -> CoverSources<'_> {
    CoverSources {
        scene,
        layout: placement.layout,
        fade: placement.fade,
        wash: placement.wash,
    }
}

#[must_use]
pub(crate) fn cover_wash(progress: Option<f32>, screen_width: u16) -> CoverWash {
    progress.map_or(CoverWash::Idle, |progress| CoverWash::Running {
        progress,
        screen_width,
    })
}

pub(crate) fn track_changed_fade(
    current: CoverFadePermission,
    cues: &[Cue],
    current_track: Option<&Path>,
) -> CoverFadePermission {
    if !cues.contains(&Cue::TrackChanged) {
        return current;
    }
    current_track.map_or(CoverFadePermission::Absent, |path| {
        CoverFadePermission::Awaiting(path.to_path_buf())
    })
}

pub(crate) fn cover_arrived_fade(
    current: CoverFadePermission,
    path: &Path,
    arrival: CoverArrival,
) -> CoverFadePermission {
    let CoverFadePermission::Awaiting(awaited) = &current else {
        return current;
    };
    if awaited != path {
        return current;
    }
    match arrival {
        CoverArrival::Decoded => CoverFadePermission::Ready(path.to_path_buf()),
        CoverArrival::Missing => CoverFadePermission::Absent,
    }
}

pub(crate) fn resolved_cover_fade(
    current: CoverFadePermission,
) -> (CoverFadePermission, CoverFade) {
    match current {
        CoverFadePermission::Ready(_) => {
            (CoverFadePermission::Absent, CoverFade::Allowed)
        }
        other @ (CoverFadePermission::Absent | CoverFadePermission::Awaiting(_)) => {
            (other, CoverFade::Withheld)
        }
    }
}

pub(crate) struct CoverWanted<'a> {
    pub(crate) current: Option<&'a Path>,
    pub(crate) style: CoverStyle,
    pub(crate) side: u32,
}

pub(crate) fn desired_cover(
    wanted: &mut Option<PathBuf>,
    sources: &CoverWanted<'_>,
) -> Option<CoverRequest> {
    if !matches!(sources.style, CoverStyle::Plain | CoverStyle::Vinyl) {
        *wanted = None;
        return None;
    }
    let Some(path) = sources.current else {
        *wanted = None;
        return None;
    };
    if wanted.as_deref() == Some(path) {
        return None;
    }
    *wanted = Some(path.to_path_buf());
    Some(CoverRequest {
        path: path.to_path_buf(),
        side: sources.side,
    })
}

pub(crate) fn cover_outcome(
    decoded: CoverDecoded,
) -> Result<Option<DecodedCover>, ShellFailure> {
    match decoded.outcome {
        CoverOutcome::Art(image) => Ok(Some(DecodedCover {
            path: decoded.path,
            image,
        })),
        CoverOutcome::NoArt => Ok(None),
        CoverOutcome::Failed(error) => Err(ShellFailure::Cover(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use config::CoverStyle;
    use image::RgbaImage;
    use kernel::Cue;
    use runtime::{CoverDecoded, CoverError, CoverOutcome};
    use terminal::{CoverFade, CoverWash};

    use crate::{
        shell::cover_fade::{
            CoverArrival,
            CoverFadePermission,
            CoverWanted,
            cover_arrived_fade,
            cover_outcome,
            cover_wash,
            desired_cover,
            resolved_cover_fade,
            track_changed_fade,
        },
        toast::ShellFailure,
    };

    #[test]
    fn an_off_style_wants_no_cover_and_forgets_it() {
        let mut wanted = Some(PathBuf::from("/music/old.jpg"));

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: Some(Path::new("/music/old.jpg")),
                style: CoverStyle::Off,
                side: 160,
            },
        );

        assert_eq!(request, None);
        assert_eq!(wanted, None);
    }

    #[test]
    fn a_milkdrop_style_wants_no_decoded_cover() {
        let mut wanted = None;

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: Some(Path::new("/music/track.jpg")),
                style: CoverStyle::Milkdrop,
                side: 160,
            },
        );

        assert_eq!(request, None);
    }

    #[test]
    fn no_current_track_wants_no_cover() {
        let mut wanted = None;

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: None,
                style: CoverStyle::Plain,
                side: 160,
            },
        );

        assert_eq!(request, None);
    }

    #[test]
    fn playback_stopping_forgets_the_wanted_cover() {
        let mut wanted = Some(PathBuf::from("/music/old.jpg"));

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: None,
                style: CoverStyle::Plain,
                side: 160,
            },
        );

        assert_eq!(request, None);
        assert_eq!(wanted, None);
    }

    #[test]
    fn a_new_track_path_is_requested_at_the_configured_side() {
        let mut wanted = None;

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: Some(Path::new("/music/track.jpg")),
                style: CoverStyle::Vinyl,
                side: 320,
            },
        );

        assert_eq!(
            request.map(|request| (request.path, request.side)),
            Some((PathBuf::from("/music/track.jpg"), 320))
        );
        assert_eq!(wanted, Some(PathBuf::from("/music/track.jpg")));
    }

    #[test]
    fn the_same_track_path_is_not_requested_again() {
        let mut wanted = Some(PathBuf::from("/music/track.jpg"));

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: Some(Path::new("/music/track.jpg")),
                style: CoverStyle::Plain,
                side: 160,
            },
        );

        assert_eq!(request, None);
    }

    #[test]
    fn a_changed_track_path_is_requested_again() {
        let mut wanted = Some(PathBuf::from("/music/old.jpg"));

        let request = desired_cover(
            &mut wanted,
            &CoverWanted {
                current: Some(Path::new("/music/new.jpg")),
                style: CoverStyle::Plain,
                side: 160,
            },
        );

        assert_eq!(
            request.map(|request| request.path),
            Some(PathBuf::from("/music/new.jpg"))
        );
        assert_eq!(wanted, Some(PathBuf::from("/music/new.jpg")));
    }

    #[test]
    fn decoded_art_becomes_a_decoded_cover() {
        let image = RgbaImage::new(2, 2);
        let decoded = CoverDecoded {
            path: PathBuf::from("/music/track.jpg"),
            side: 64,
            outcome: CoverOutcome::Art(image.clone()),
        };

        let mapped = cover_outcome(decoded);

        assert_eq!(
            mapped.map(|cover| cover.map(|cover| (cover.path, cover.image))),
            Ok(Some((PathBuf::from("/music/track.jpg"), image)))
        );
    }

    #[test]
    fn no_art_becomes_no_decoded_cover() {
        let decoded = CoverDecoded {
            path: PathBuf::from("/music/track.jpg"),
            side: 64,
            outcome: CoverOutcome::NoArt,
        };

        assert!(matches!(cover_outcome(decoded), Ok(None)));
    }

    fn broken_cover_error() -> CoverError {
        CoverError {
            source: image::load_from_memory(b"not an image").unwrap_err(),
        }
    }

    #[test]
    fn a_failed_decode_becomes_a_cover_failure_at_once() {
        let decoded = CoverDecoded {
            path: PathBuf::from("/music/track.jpg"),
            side: 64,
            outcome: CoverOutcome::Failed(broken_cover_error()),
        };

        assert!(matches!(
            cover_outcome(decoded),
            Err(ShellFailure::Cover(reason))
                if reason == broken_cover_error().to_string()
        ));
    }

    #[test]
    fn a_track_change_with_a_current_track_awaits_its_cover() {
        let fade = track_changed_fade(
            CoverFadePermission::Absent,
            &[Cue::TrackChanged],
            Some(Path::new("/music/new.jpg")),
        );

        assert_eq!(
            fade,
            CoverFadePermission::Awaiting(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_resize_with_no_track_change_leaves_the_permission_untouched() {
        let fade = track_changed_fade(
            CoverFadePermission::Absent,
            &[],
            Some(Path::new("/music/old.jpg")),
        );

        assert_eq!(fade, CoverFadePermission::Absent);
        assert_eq!(resolved_cover_fade(fade).1, CoverFade::Withheld);
    }

    #[test]
    fn a_decoded_cover_for_the_awaited_track_becomes_ready() {
        let fade = cover_arrived_fade(
            CoverFadePermission::Awaiting(PathBuf::from("/music/new.jpg")),
            Path::new("/music/new.jpg"),
            CoverArrival::Decoded,
        );

        assert_eq!(
            fade,
            CoverFadePermission::Ready(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_decoded_cover_for_a_different_track_is_ignored() {
        let fade = cover_arrived_fade(
            CoverFadePermission::Awaiting(PathBuf::from("/music/new.jpg")),
            Path::new("/music/other.jpg"),
            CoverArrival::Decoded,
        );

        assert_eq!(
            fade,
            CoverFadePermission::Awaiting(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_missing_cover_for_the_awaited_track_drops_the_permission() {
        let fade = cover_arrived_fade(
            CoverFadePermission::Awaiting(PathBuf::from("/music/new.jpg")),
            Path::new("/music/new.jpg"),
            CoverArrival::Missing,
        );

        assert_eq!(fade, CoverFadePermission::Absent);
    }

    #[test]
    fn a_ready_permission_is_allowed_once_then_dropped() {
        let (next, fade) = resolved_cover_fade(CoverFadePermission::Ready(
            PathBuf::from("/music/new.jpg"),
        ));

        assert_eq!(fade, CoverFade::Allowed);
        assert_eq!(next, CoverFadePermission::Absent);
    }

    #[test]
    fn an_awaiting_permission_is_withheld_and_kept() {
        let (next, fade) = resolved_cover_fade(CoverFadePermission::Awaiting(
            PathBuf::from("/music/new.jpg"),
        ));

        assert_eq!(fade, CoverFade::Withheld);
        assert_eq!(
            next,
            CoverFadePermission::Awaiting(PathBuf::from("/music/new.jpg"))
        );
    }

    #[test]
    fn a_track_changes_then_its_cover_arrives_two_paints_later() {
        let fade = track_changed_fade(
            CoverFadePermission::Absent,
            &[Cue::TrackChanged],
            Some(Path::new("/music/new.jpg")),
        );
        let (fade, first_paint) = resolved_cover_fade(fade);
        assert_eq!(first_paint, CoverFade::Withheld);

        let fade = track_changed_fade(fade, &[], Some(Path::new("/music/new.jpg")));
        let (fade, second_paint) = resolved_cover_fade(fade);
        assert_eq!(second_paint, CoverFade::Withheld);

        let fade = cover_arrived_fade(
            fade,
            Path::new("/music/new.jpg"),
            CoverArrival::Decoded,
        );
        let (fade, third_paint) = resolved_cover_fade(fade);
        assert_eq!(third_paint, CoverFade::Allowed);

        let (_, fourth_paint) = resolved_cover_fade(fade);
        assert_eq!(fourth_paint, CoverFade::Withheld);
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
