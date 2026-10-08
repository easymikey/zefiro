use std::time::Duration;

use kernel::domain::{appearance::Animations, cue::Cue};
use ratatui::{buffer::Buffer, layout::Rect};
use strum::IntoEnumIterator;
use widgets::{
    animation::stage::{AnimationStage, Backdrop},
    screen::frame_layout::FrameLayout,
};

use crate::support::{
    AREA,
    COVER,
    PROGRESS_LINE,
    animation_frame,
    overlay_backdrop,
    pane_backdrop,
    quiet_backdrop,
    screen_backdrop,
    screen_frame,
    slice,
    toast_card_backdrop,
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
        if !stage.is_animating() {
            return last;
        }
        last = step_over(stage, frame, Duration::from_millis(33));
    }
    last
}

pub(crate) fn has_moved(before: &Buffer, after: &Buffer, rect: Rect) -> bool {
    (rect.y..rect.bottom()).any(|row| {
        (rect.x..rect.right())
            .any(|column| before.cell((column, row)) != after.cell((column, row)))
    })
}

#[test]
fn every_cue_with_animations_off_stages_nothing_and_drops_what_was_running() {
    for cue in Cue::iter() {
        let mut stage = AnimationStage::default();
        stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
        assert!(
            stage.is_animating(),
            "sanity: {cue:?} follows a running ring"
        );

        stage.play(
            vec![cue],
            &Backdrop {
                animations: Animations::Off,
                ..pane_backdrop()
            },
        );

        assert!(
            !stage.is_animating(),
            "{cue:?} played while animations are off"
        );
    }
}

#[test]
fn elapsed_is_the_clock_delta_while_an_animation_is_running() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
    assert!(stage.is_animating(), "sanity: something needs the delta");

    assert_eq!(
        stage.advance_to(Duration::from_millis(100)),
        Duration::from_millis(100)
    );
    assert_eq!(
        stage.advance_to(Duration::from_millis(133)),
        Duration::from_millis(33)
    );
    assert_eq!(stage.advance_to(Duration::from_millis(100)), Duration::ZERO);
}

#[test]
fn an_idle_gap_is_not_charged_to_the_animation_the_next_frame_stages() {
    let mut stage = AnimationStage::default();
    let first = stage.advance_to(Duration::ZERO);
    stage.play(Vec::new(), &overlay_backdrop(None));
    let mut buffer = animation_frame();
    stage.advance(&mut buffer, first);
    assert!(!stage.is_animating(), "sanity: an idle, empty stage");

    let gap = stage.advance_to(Duration::from_secs(4));
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(AREA)));
    assert_eq!(gap, Duration::ZERO, "nothing was running to step");

    let opened = step(&mut stage, gap);
    assert_ne!(opened, animation_frame(), "the ring is on its first frame");
    assert!(
        stage.is_animating(),
        "an idle gap must not run the transition out inside one frame"
    );
}

#[test]
fn a_cue_with_no_painted_rect_stages_nothing() {
    let mut stage = AnimationStage::default();
    stage.play(
        vec![Cue::FavoriteToggled, Cue::VolumeChanged],
        &quiet_backdrop(),
    );

    assert!(!stage.is_animating());
}

#[test]
fn only_the_cover_rect_survives_a_whole_screen_animation() {
    let mut opened = overlay_backdrop(Some(crate::support::SCREEN));
    opened.layout.cover_area = Some(COVER);

    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &opened);

    let original = screen_frame();
    let mut buffer = screen_frame();
    stage.advance(&mut buffer, slice(|t| t.modal_reveal, 2));

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
        has_moved(&original, &buffer, PROGRESS_LINE),
        "the progress rect now takes part in the wash"
    );
    assert_ne!(buffer, original, "everything else still animates");
}

#[test]
fn a_cover_arriving_mid_animation_is_still_protected() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::ThemeChanged], &screen_backdrop());
    assert!(stage.is_animating(), "the wash is under way");
    step_over(&mut stage, screen_frame, slice(|t| t.screen_wash, 4));

    stage.play(
        Vec::new(),
        &Backdrop {
            layout: FrameLayout {
                cover_area: Some(COVER),
                ..screen_backdrop().layout
            },
            ..screen_backdrop()
        },
    );

    let original = screen_frame();
    let frame = step_over(&mut stage, screen_frame, slice(|t| t.screen_wash, 4));
    assert!(
        !has_moved(&original, &frame, COVER),
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
    run_out_over(&mut stage, screen_frame);
    stage.play(vec![Cue::ToastDismissed], &toast_card_backdrop());
    assert!(
        stage.is_animating(),
        "sanity: the toast leaves with a burst"
    );

    let last = step_over(&mut stage, screen_frame, whole(|t| t.delete_burst));

    assert_eq!(last, screen_frame(), "the burst ends on the painted frame");
    assert!(
        stage.is_animating(),
        "the tick after the last animation still asks for a frame"
    );
    stage.play(Vec::new(), &toast_card_backdrop());
    let settled = step_over(&mut stage, screen_frame, Duration::ZERO);
    assert_eq!(settled, screen_frame());
    assert!(
        !stage.is_animating(),
        "and once that frame is painted the stage settles"
    );
}
