use std::time::Duration;

use config::Animations;
use kernel::Cue;
use ratatui::{buffer::Buffer, layout::Rect};
use strum::IntoEnumIterator;
use widgets::{AnimationStage, Backdrop, FrameLayout};

use crate::unit::support::{
    ACCENT,
    AREA,
    BACKGROUND,
    COVER,
    PROGRESS_LINE,
    Scenery,
    ToastPresence,
    animation_frame,
    model_with_tracks,
    overlay_backdrop,
    pane_backdrop,
    quiet_backdrop,
    screen_backdrop,
    screen_frame,
    slice,
    toast_backdrop,
    toast_card_backdrop,
    volume_fill,
    volume_lifted,
    whole,
};

pub(crate) fn step_over(
    stage: &mut AnimationStage,
    frame: fn() -> Buffer,
    elapsed: Duration,
) -> Buffer {
    let mut buffer = frame();
    stage.advance(&mut buffer, elapsed);
    buffer
}

pub(crate) fn step(stage: &mut AnimationStage, elapsed: Duration) -> Buffer {
    step_over(stage, animation_frame, elapsed)
}

pub(crate) fn run_out_over(
    stage: &mut AnimationStage,
    frame: fn() -> Buffer,
) -> Buffer {
    let mut last = frame();
    for _ in 0..256 {
        if !stage.is_running() {
            return last;
        }
        last = step_over(stage, frame, Duration::from_millis(33));
    }
    last
}

pub(crate) fn moved(before: &Buffer, after: &Buffer, rect: Rect) -> bool {
    (rect.y..rect.bottom()).any(|row| {
        (rect.x..rect.right())
            .any(|column| before.cell((column, row)) != after.cell((column, row)))
    })
}

#[test]
fn an_opening_overlay_resolves_and_ends_on_the_painted_colours() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
    assert!(stage.is_running(), "opening an overlay stages the ring");

    assert_ne!(step(&mut stage, Duration::ZERO), animation_frame());
    assert_ne!(
        step(&mut stage, slice(|t| t.modal_in, 2)),
        animation_frame()
    );
    assert!(stage.is_running(), "half way through");

    assert_eq!(
        step(&mut stage, slice(|t| t.modal_in, 2)),
        animation_frame()
    );
    assert!(!stage.is_running(), "a finished animation is dropped");
}

#[test]
fn a_closing_overlay_resolves_the_rect_it_vacated() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
    let _ = step(&mut stage, whole(|t| t.modal_in));
    assert!(!stage.is_running(), "the open ring finished");

    stage.play(vec![Cue::OverlayClosed], &overlay_backdrop(None));

    assert!(stage.is_running(), "closing stages its own");
    assert_ne!(step(&mut stage, Duration::ZERO), animation_frame());
    assert_eq!(step(&mut stage, whole(|t| t.modal_out)), animation_frame());
    assert!(!stage.is_running(), "and it ends");
}

#[test]
fn a_frame_without_cues_stages_nothing() {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), &overlay_backdrop(Some(AREA)));

    assert!(!stage.is_running());
}

#[test]
fn every_cue_with_animations_off_stages_nothing_and_drops_what_was_running() {
    for cue in Cue::iter() {
        let mut stage = AnimationStage::default();
        stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
        assert!(stage.is_running(), "sanity: {cue:?} follows a running ring");

        stage.play(
            vec![cue],
            &Backdrop {
                animations: Animations::Off,
                ..pane_backdrop()
            },
        );

        assert!(
            !stage.is_running(),
            "{cue:?} played while animations are off"
        );
    }
}

#[test]
fn elapsed_is_the_clock_delta_while_an_animation_is_running() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
    assert!(stage.is_running(), "sanity: something needs the delta");

    assert_eq!(
        stage.advance_clock(Duration::from_millis(100)),
        Duration::from_millis(100)
    );
    assert_eq!(
        stage.advance_clock(Duration::from_millis(133)),
        Duration::from_millis(33)
    );
    assert_eq!(
        stage.advance_clock(Duration::from_millis(100)),
        Duration::ZERO
    );
}

#[test]
fn an_idle_gap_is_not_charged_to_the_animation_the_next_frame_stages() {
    let mut stage = AnimationStage::default();
    let first = stage.advance_clock(Duration::ZERO);
    stage.play(Vec::new(), &overlay_backdrop(None));
    let mut buffer = animation_frame();
    stage.advance(&mut buffer, first);
    assert!(!stage.is_running(), "sanity: an idle, empty stage");

    let gap = stage.advance_clock(Duration::from_secs(4));
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
    assert_eq!(gap, Duration::ZERO, "nothing was running to step");

    let opened = step(&mut stage, gap);
    assert_ne!(opened, animation_frame(), "the ring is on its first frame");
    assert!(
        stage.is_running(),
        "an idle gap must not run the transition out inside one frame"
    );
}

#[test]
fn a_track_change_stages_nothing_over_the_card() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::TrackChanged], &pane_backdrop());

    assert!(!stage.is_running(), "the card switches without animating");
}

#[test]
fn the_same_cue_twice_in_one_frame_stages_it_once() {
    let mut doubled = AnimationStage::default();
    doubled.play(
        vec![Cue::FavoriteToggled, Cue::FavoriteToggled],
        &pane_backdrop(),
    );

    let mut single = AnimationStage::default();
    single.play(vec![Cue::FavoriteToggled], &pane_backdrop());

    assert_eq!(
        doubled.staged_count(),
        single.staged_count(),
        "one batch of identical cues is one visible change"
    );
}

#[test]
fn a_frame_with_no_cues_leaves_a_running_animation_alone() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::FavoriteToggled], &pane_backdrop());
    let staged = stage.staged_count();

    stage.play(Vec::new(), &pane_backdrop());

    assert_eq!(
        stage.staged_count(),
        staged,
        "nothing was cued, nothing changes"
    );
}

#[test]
fn a_cue_with_no_painted_rect_stages_nothing() {
    let mut stage = AnimationStage::default();
    stage.play(
        vec![Cue::FavoriteToggled, Cue::VolumeChanged],
        &quiet_backdrop(),
    );

    assert!(!stage.is_running());
}

#[test]
fn a_protocol_cover_rect_is_subtracted_from_the_fade() {
    let cover = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 1,
    };
    let mut opened = overlay_backdrop(Some(AREA));
    opened.layout.cover = Some(cover);
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &opened);

    let faded = step(&mut stage, Duration::ZERO);
    let original = animation_frame();
    for column in 0..cover.width {
        assert_eq!(
            faded.cell((column, 0)),
            original.cell((column, 0)),
            "column {column} is the cover's"
        );
    }
    assert_ne!(
        faded.cell((cover.width, 0)),
        original.cell((cover.width, 0))
    );
}

#[test]
fn only_the_cover_rect_survives_a_whole_screen_animation() {
    let mut opened = overlay_backdrop(Some(crate::unit::support::SCREEN));
    opened.layout.cover = Some(COVER);

    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &opened);

    let original = screen_frame();
    let mut buffer = screen_frame();
    stage.advance(&mut buffer, slice(|t| t.modal_in, 2));

    for y in COVER.y..COVER.bottom() {
        for x in COVER.x..COVER.right() {
            assert_eq!(
                buffer.cell((x, y)),
                original.cell((x, y)),
                "({x}, {y}) belongs to a graphics protocol"
            );
        }
    }
    assert!(
        moved(&original, &buffer, PROGRESS_LINE),
        "the progress rect now takes part in the wash"
    );
    assert_ne!(buffer, original, "everything else still animates");
}

#[test]
fn a_theme_wash_runs_and_ends_on_the_painted_frame() {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), &screen_backdrop());
    assert!(!stage.is_running(), "an empty library assembles nothing");

    stage.play(vec![Cue::ThemeChanged], &screen_backdrop());
    assert!(stage.is_running(), "a new theme washes over the screen");
    assert_ne!(
        step_over(&mut stage, screen_frame, slice(|t| t.screen_wash, 4)),
        screen_frame()
    );
    assert_eq!(run_out_over(&mut stage, screen_frame), screen_frame());
    assert!(!stage.is_running());
}

#[test]
fn a_cover_arriving_mid_animation_is_still_protected() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::ThemeChanged], &screen_backdrop());
    assert!(stage.is_running(), "the wash is under way");
    let _ = step_over(&mut stage, screen_frame, slice(|t| t.screen_wash, 4));

    stage.play(
        Vec::new(),
        &Backdrop {
            layout: FrameLayout {
                cover: Some(COVER),
                ..screen_backdrop().layout
            },
            ..screen_backdrop()
        },
    );

    let original = screen_frame();
    let frame = step_over(&mut stage, screen_frame, slice(|t| t.screen_wash, 4));
    assert!(
        !moved(&original, &frame, COVER),
        "the cover arrived mid-animation and must be left alone from that frame on"
    );
    assert_ne!(
        frame, original,
        "sanity: the rest of the screen is still animating"
    );
}

#[test]
fn the_frame_after_the_last_animation_is_asked_for_so_the_row_it_covered_comes_back() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::ToastRaised], &toast_card_backdrop());
    let _ = run_out_over(&mut stage, screen_frame);
    stage.play(vec![Cue::ToastDismissed], &toast_card_backdrop());
    assert!(stage.is_running(), "sanity: the toast leaves with a burst");

    let last = run_out_over(&mut stage, screen_frame);

    assert_eq!(last, screen_frame(), "the burst ends on the painted frame");
    assert!(
        stage.wants_frame(),
        "the tick after the last animation still asks for a frame"
    );
    stage.play(Vec::new(), &toast_card_backdrop());
    let settled = step_over(&mut stage, screen_frame, Duration::ZERO);
    assert_eq!(settled, screen_frame());
    assert!(
        !stage.wants_frame(),
        "and once that frame is painted the stage settles"
    );
}

#[test]
fn a_toast_slides_in_and_its_row_comes_back_when_it_expires() {
    let mut stage = AnimationStage::default();

    stage.play(
        vec![Cue::ToastRaised],
        &toast_backdrop(ToastPresence::Shown),
    );
    assert!(stage.is_running(), "a toast arriving stages a slide");
    assert_ne!(step(&mut stage, Duration::ZERO), animation_frame());
    assert_eq!(
        step(&mut stage, whole(|t| t.toast_slide_in)),
        animation_frame()
    );
    assert!(!stage.is_running());

    stage.play(
        vec![Cue::ToastDismissed],
        &toast_backdrop(ToastPresence::Hidden),
    );
    assert!(stage.is_running(), "a toast expiring bursts apart");
    assert_ne!(
        step(&mut stage, slice(|t| t.delete_burst, 2)),
        animation_frame(),
        "halfway out the row is still moving"
    );
    let _ = step(&mut stage, whole(|t| t.delete_burst));
    assert!(!stage.is_running());
    assert_eq!(
        step(&mut stage, Duration::ZERO),
        animation_frame(),
        "the row it covered comes back once the burst is over"
    );
}

#[test]
fn the_toast_burst_is_the_same_every_time() {
    let halfway = |()| {
        let mut stage = AnimationStage::default();
        stage.play(
            vec![Cue::ToastRaised],
            &toast_backdrop(ToastPresence::Shown),
        );
        let _ = step(&mut stage, whole(|t| t.toast_slide_in));
        stage.play(
            vec![Cue::ToastDismissed],
            &toast_backdrop(ToastPresence::Hidden),
        );
        step(&mut stage, slice(|t| t.delete_burst, 2))
    };

    assert_eq!(halfway(()), halfway(()));
}

#[test]
fn a_second_toast_while_one_is_showing_slides_in_again() {
    let mut stage = AnimationStage::default();
    stage.play(
        vec![Cue::ToastRaised],
        &toast_backdrop(ToastPresence::Shown),
    );
    let _ = step(&mut stage, whole(|t| t.toast_slide_in));
    assert!(!stage.is_running(), "sanity: the first slide finished");

    stage.play(
        vec![Cue::ToastRaised],
        &toast_backdrop(ToastPresence::Shown),
    );

    assert!(stage.is_running(), "the replacing toast slides in too");
}

#[test]
fn the_stage_animates_frame_layout_rects_as_the_scenes_clock_advances() {
    let sources = Scenery::new(model_with_tracks(3));
    let scene = sources.scene();
    let layout = FrameLayout::new(&scene.layout_inputs(), crate::unit::support::SCREEN);

    let backdrop = Backdrop {
        animations: Animations::On,
        layout,
        background: BACKGROUND,
        accent: ACCENT,
        volume_fill: volume_fill(),
        volume_lifted: volume_lifted(),
        wash_from: BACKGROUND,
    };

    let mut stage = AnimationStage::default();
    let start = stage.advance_clock(scene.clock);
    stage.play(vec![Cue::ThemeChanged], &backdrop);
    assert!(
        stage.is_running(),
        "a theme change washes the real frame layout"
    );

    let mut buffer = Buffer::empty(crate::unit::support::SCREEN);
    stage.advance(&mut buffer, start);
    let mid = scene.clock + slice(|t| t.screen_wash, 4);
    let elapsed = stage.advance_clock(mid);
    stage.advance(&mut buffer, elapsed);
    assert!(
        stage.is_running(),
        "the wash is still under way midway through, driven only by the scene's clock"
    );

    let same_reading = stage.advance_clock(mid);
    assert_eq!(
        same_reading,
        Duration::ZERO,
        "reading the same clock value again charges nothing"
    );
}
