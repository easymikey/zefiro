use std::time::Duration;

use kernel::domain::cue::Cue;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};
use rstest::rstest;
use tachyonfx::{CellFilter, Effect as Animation};
use widgets::animation::{
    catalogue::{
        VolumeShades,
        chip_pulse,
        modal_in,
        row_flash,
        scatter_burst,
        screen_wash,
        toast_slide_in,
        volume_pulse,
        wash_reveal,
    },
    stage::AnimationStage,
};

use crate::unit::{
    animation_stage::{run_out_over, step_over},
    support::{
        ACCENT,
        BACKGROUND,
        SCREEN,
        TEXT,
        TOAST_CARD,
        overlay_backdrop,
        screen_frame,
        slice,
        toast_card_backdrop,
        volume_fill,
        volume_lifted,
    },
};

const PAST_THE_END: Duration = Duration::from_secs(10);

fn storyboard_frame(frame: &Buffer, painted: &Buffer) -> String {
    let mut out = String::new();
    for row in 0..SCREEN.height {
        for column in 0..SCREEN.width {
            let symbol = frame.cell((column, row)).map_or(" ", |cell| cell.symbol());
            out.push_str(if symbol.is_empty() { " " } else { symbol });
        }
        out.push_str("  |  ");
        for column in 0..SCREEN.width {
            let same = frame.cell((column, row)) == painted.cell((column, row));
            out.push(if same { '.' } else { '#' });
        }
        out.push('\n');
    }
    out
}

fn storyboard(stage: &mut AnimationStage, mid: Duration) -> String {
    let painted = screen_frame();
    let opened = step_over(stage, screen_frame, Duration::ZERO);
    let middle = step_over(stage, screen_frame, mid);
    let ending = run_out_over(stage, screen_frame);
    format!(
        "t=0\n{}\nmid\n{}\nend\n{}",
        storyboard_frame(&opened, &painted),
        storyboard_frame(&middle, &painted),
        storyboard_frame(&ending, &painted),
    )
}

const MODAL: Rect = Rect {
    x: 4,
    y: 1,
    width: 16,
    height: 4,
};

#[test]
fn modal_open_storyboard() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::OverlayOpened], &overlay_backdrop(Some(MODAL)));

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.modal_in, 2)));
}

#[test]
fn modal_close_storyboard() {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), &overlay_backdrop(Some(MODAL)));
    stage.play(vec![Cue::OverlayClosed], &overlay_backdrop(None));

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.modal_in, 2)));
}

#[test]
fn toast_slide_in_storyboard() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::ToastRaised], &toast_card_backdrop());

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.toast_slide_in, 2)));
}

fn strayed_outside(frame: &Buffer, painted: &Buffer) -> Vec<(u16, u16)> {
    let mut strayed = Vec::new();
    for row in 0..SCREEN.height {
        for column in 0..SCREEN.width {
            let inside = TOAST_CARD.contains(Position { x: column, y: row });
            if !inside && frame.cell((column, row)) != painted.cell((column, row)) {
                strayed.push((column, row));
            }
        }
    }
    strayed
}

#[test]
fn the_arriving_toast_paints_nothing_outside_the_card() {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), &toast_card_backdrop());
    stage.play(vec![Cue::ToastRaised], &toast_card_backdrop());

    let painted = screen_frame();
    let mut guard = 0;
    while stage.is_animating() {
        let frame = step_over(&mut stage, screen_frame, Duration::from_millis(33));
        assert_eq!(
            strayed_outside(&frame, &painted),
            Vec::new(),
            "the slide reveals inside the card rect and nowhere else"
        );
        guard += 1;
        assert!(guard < 64, "the slide must run out");
    }
}

#[test]
fn delete_burst_storyboard() {
    let mut stage = AnimationStage::default();
    let pane = crate::unit::support::pane_backdrop();
    stage.play(Vec::new(), &pane);
    stage.play(vec![Cue::TrackDeleted], &pane);

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.delete_burst, 2)));
}

fn every_animation() -> Vec<(String, Animation)> {
    vec![
        ("modal_in".to_string(), modal_in()),
        ("toast_slide_in".to_string(), toast_slide_in(BACKGROUND)),
        ("chip_pulse".to_string(), chip_pulse(ACCENT)),
        ("row_flash".to_string(), row_flash(ACCENT)),
        (
            "volume_pulse".to_string(),
            volume_pulse(
                VolumeShades {
                    fill: volume_fill(),
                    lifted: volume_lifted(),
                },
                CellFilter::All,
            ),
        ),
        ("screen_wash".to_string(), screen_wash(BACKGROUND)),
        (
            "scatter_burst".to_string(),
            scatter_burst(BACKGROUND, CellFilter::All),
        ),
    ]
}

#[test]
fn no_animation_outlives_its_own_timer() {
    for (name, animation) in every_animation() {
        let mut stage = AnimationStage::default();
        stage.stage(animation, SCREEN);
        for _ in 0..3 {
            step_over(&mut stage, screen_frame, PAST_THE_END);
        }
        assert!(!stage.is_animating(), "{name} outlived its own timer");
    }
}

#[test]
fn every_animation_ends_on_the_painted_frame() {
    for (name, animation) in every_animation() {
        let mut stage = AnimationStage::default();
        stage.stage(animation, SCREEN);
        assert_eq!(
            run_out_over(&mut stage, screen_frame),
            screen_frame(),
            "{name} left the frame changed after it finished"
        );
    }
}

#[rstest]
#[case::the_first_column(0)]
#[case::a_middle_column(40)]
#[case::the_last_column(89)]
fn wash_reveal_answers_nothing_at_the_start(#[case] column: u16) {
    assert_eq!(wash_reveal(0.0, column, 90), 0.0);
}

#[rstest]
#[case::the_first_column(0)]
#[case::a_middle_column(40)]
#[case::the_last_column(89)]
fn wash_reveal_answers_everything_at_the_end(#[case] column: u16) {
    assert_eq!(wash_reveal(1.0, column, 90), 1.0);
}

#[test]
fn wash_reveal_is_never_smaller_for_a_column_further_left() {
    let left = wash_reveal(0.5, 10, 90);
    let right = wash_reveal(0.5, 80, 90);
    assert!(
        left > right,
        "left={left} must reveal ahead of right={right}"
    );
}

fn filled_row(width: u16) -> Buffer {
    let area = Rect {
        x: 0,
        y: 0,
        width,
        height: 1,
    };
    let mut buffer = Buffer::empty(area);
    buffer.set_string(
        0,
        0,
        "X".repeat(usize::from(width)),
        Style::default().fg(TEXT).bg(BACKGROUND),
    );
    buffer
}

fn wash_row(buffer: &Buffer, width: u16) -> String {
    (0..width)
        .map(|column| {
            let bg = buffer.cell((column, 0)).map(|cell| cell.bg);
            if bg == Some(BACKGROUND) {
                'R'
            } else if bg == Some(ACCENT) {
                '.'
            } else {
                '~'
            }
        })
        .collect()
}

#[test]
fn a_theme_wash_reveals_left_before_right_mid_animation() {
    let width = 90;
    let mut stage = AnimationStage::default();
    stage.stage(
        screen_wash(ACCENT),
        Rect {
            x: 0,
            y: 0,
            width,
            height: 1,
        },
    );

    let mut buffer = filled_row(width);
    stage.advance(&mut buffer, slice(|t| t.screen_wash, 4));

    insta::assert_snapshot!(wash_row(&buffer, width));
}
