use kernel::domain::cue::Cue;
use ratatui::layout::Rect;
use rstest::rstest;
use widgets::animation::stage::{AnimationStage, Backdrop};

use crate::unit::{
    animation_stage::{has_moved, run_out_over, step_over},
    support::{
        CARD_TITLE,
        PANE_ROW,
        PANE_STATUS,
        SCREEN,
        VOLUME_LABEL,
        pane_backdrop,
        pane_star,
        screen_backdrop,
        slice,
        volume_bar_frame,
    },
};

fn animated(cues: &[Cue], backdrop: &Backdrop, watched: &[Rect]) -> (bool, Vec<Rect>) {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), backdrop);
    run_out_over(&mut stage, volume_bar_frame);

    stage.play(cues.to_vec(), backdrop);
    let is_animating = stage.is_animating();
    let painted = volume_bar_frame();
    let opened = step_over(&mut stage, volume_bar_frame, slice(|t| t.screen_wash, 4));
    let moved_rects = watched
        .iter()
        .filter(|rect| has_moved(&painted, &opened, **rect))
        .copied()
        .collect();
    if is_animating {
        assert_eq!(
            run_out_over(&mut stage, volume_bar_frame),
            painted,
            "every animation ends on exactly the frame underneath it"
        );
    }
    (is_animating, moved_rects)
}

struct AnimationRow<'a> {
    cues: &'a [Cue],
    backdrop: Backdrop,
    expected: &'a [Rect],
    still: &'a [Rect],
}

#[rstest]
#[case::a_queue_add(AnimationRow {
    cues: &[Cue::QueueChanged],
    backdrop: pane_backdrop(),
    expected: &[],
    still: &[PANE_STATUS, PANE_ROW],
})]
#[case::a_track_change(AnimationRow {
    cues: &[Cue::TrackChanged],
    backdrop: pane_backdrop(),
    expected: &[],
    still: &[CARD_TITLE, PANE_STATUS],
})]
#[case::a_favorite_toggle(AnimationRow {
    cues: &[Cue::FavoriteToggled],
    backdrop: pane_backdrop(),
    expected: &[pane_star()],
    still: &[],
})]
#[case::a_cursor_move_onto_another_row(AnimationRow {
    cues: &[],
    backdrop: pane_backdrop(),
    expected: &[],
    still: &[pane_star(), PANE_ROW],
})]
#[case::shuffle(AnimationRow {
    cues: &[Cue::PlayOrderChanged],
    backdrop: pane_backdrop(),
    expected: &[],
    still: &[PANE_STATUS, PANE_ROW],
})]
#[case::a_library_opening(AnimationRow {
    cues: &[Cue::LibraryOpened],
    backdrop: screen_backdrop(),
    expected: &[],
    still: &[SCREEN],
})]
#[case::a_volume_change(AnimationRow {
    cues: &[Cue::VolumeChanged],
    backdrop: pane_backdrop(),
    expected: &[VOLUME_LABEL],
    still: &[PANE_STATUS],
})]
#[case::a_deleted_row(AnimationRow {
    cues: &[Cue::TrackTrashed],
    backdrop: pane_backdrop(),
    expected: &[PANE_ROW],
    still: &[],
})]
#[case::a_theme_change(AnimationRow {
    cues: &[Cue::ThemeChanged],
    backdrop: screen_backdrop(),
    expected: &[SCREEN],
    still: &[],
})]
fn an_action_animates_its_own_rect(#[case] row: AnimationRow<'_>) {
    let AnimationRow {
        cues,
        backdrop,
        expected,
        still,
    } = row;
    let watched = [
        PANE_STATUS,
        PANE_ROW,
        pane_star(),
        VOLUME_LABEL,
        CARD_TITLE,
        SCREEN,
    ];
    let (is_animating, moved_rects) = animated(cues, &backdrop, &watched);

    assert_eq!(
        is_animating,
        !expected.is_empty(),
        "a cue stages exactly when it has a rect to move"
    );
    for rect in expected {
        assert!(
            moved_rects.contains(rect),
            "{rect:?} must move, moved: {moved_rects:?}"
        );
    }
    for rect in still {
        assert!(
            !moved_rects.contains(rect),
            "{rect:?} must stay still, moved: {moved_rects:?}"
        );
    }
}

#[test]
fn a_frame_that_cues_nothing_stages_no_user_action_animations() {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), &pane_backdrop());

    assert!(!stage.is_animating(), "launching is not a change");
}
