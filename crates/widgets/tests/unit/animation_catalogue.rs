use std::{sync::Arc, time::Duration};

use kernel::domain::cue::Cue;
use ratatui::{
    buffer::{Buffer, CellDiffOption},
    layout::{Position, Rect},
    style::{Color, Style},
};
use tachyonfx::{CellFilter, Effect as Animation};
use widgets::animation::{
    catalogue::{
        PaintedCell,
        chip_pulse,
        modal_reveal,
        row_flash,
        scatter_burst,
        screen_wash,
        toast_slide_in,
        volume_pulse,
    },
    stage::AnimationStage,
};

use crate::{
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
    unit::animation_stage::{run_out_over, step_over},
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
            let is_same = frame.cell((column, row)) == painted.cell((column, row));
            out.push(if is_same { '.' } else { '#' });
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

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.modal_reveal, 2)));
}

#[test]
fn modal_close_storyboard() {
    let mut stage = AnimationStage::default();
    stage.play(Vec::new(), &overlay_backdrop(Some(MODAL)));
    stage.play(vec![Cue::OverlayClosed], &overlay_backdrop(None));

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.modal_reveal, 2)));
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
            let is_inside = TOAST_CARD.contains(Position { x: column, y: row });
            if !is_inside && frame.cell((column, row)) != painted.cell((column, row)) {
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
    let pane = crate::support::pane_backdrop();
    stage.play(Vec::new(), &pane);
    stage.play(vec![Cue::TrackTrashed], &pane);

    insta::assert_snapshot!(storyboard(&mut stage, slice(|t| t.delete_burst, 2)));
}

fn every_animation() -> Vec<(String, Animation)> {
    vec![
        ("modal_reveal".to_string(), modal_reveal()),
        ("toast_slide_in".to_string(), toast_slide_in(BACKGROUND)),
        ("chip_pulse".to_string(), chip_pulse(ACCENT)),
        ("row_flash".to_string(), row_flash(ACCENT)),
        (
            "volume_pulse".to_string(),
            volume_pulse(volume_fill(), volume_lifted(), CellFilter::All),
        ),
        ("screen_wash".to_string(), screen_wash(Arc::default())),
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

const OLD: PaintedCell = PaintedCell {
    fg: ACCENT,
    bg: TEXT,
};

const DRAWN: PaintedCell = PaintedCell {
    fg: TEXT,
    bg: BACKGROUND,
};

fn washed(from: Arc<[PaintedCell]>, mut buffer: Buffer, elapsed: Duration) -> Buffer {
    let mut stage = AnimationStage::default();
    stage.stage(screen_wash(from), buffer.area);
    stage.advance(&mut buffer, elapsed);
    buffer
}

fn washed_row(elapsed: Duration) -> Buffer {
    washed(vec![OLD; 90].into(), filled_row(90), elapsed)
}

fn row_colors(buffer: &Buffer) -> Vec<PaintedCell> {
    buffer.content.iter().map(PaintedCell::from).collect()
}

fn strictly_between(color: Color, from: Color, to: Color) -> bool {
    match (color, from, to) {
        (
            Color::Rgb(r, g, b),
            Color::Rgb(from_r, from_g, from_b),
            Color::Rgb(to_r, to_g, to_b),
        ) => [(r, from_r, to_r), (g, from_g, to_g), (b, from_b, to_b)]
            .into_iter()
            .all(|(channel, start, end)| {
                channel > start.min(end) && channel < start.max(end)
            }),
        _ => false,
    }
}

#[test]
fn a_theme_wash_fades_every_cell_at_once() {
    let start = washed_row(Duration::ZERO);
    assert_eq!(
        row_colors(&start),
        vec![OLD; 90],
        "at the start every cell shows its old colours"
    );
    assert!(
        start.content.iter().all(
            |cell| cell.diff_option == CellDiffOption::None && cell.symbol() == "X"
        ),
        "no cell is blank or left to the terminal"
    );

    let middle = row_colors(&washed_row(slice(|t| t.screen_wash, 2)));
    assert_eq!(
        middle.first(),
        middle.last(),
        "the first and the last cell fade by the same fraction"
    );
    assert!(
        middle.first().is_some_and(|first| {
            strictly_between(first.fg, OLD.fg, DRAWN.fg)
                && strictly_between(first.bg, OLD.bg, DRAWN.bg)
        }),
        "mid-animation a cell blends both colours: {:?}",
        middle.first()
    );

    let end = washed_row(slice(|t| t.screen_wash, 1));
    assert_eq!(
        row_colors(&end),
        vec![DRAWN; 90],
        "at the end every cell is drawn"
    );
}

#[test]
fn a_theme_wash_is_done_after_150_ms() {
    assert_ne!(
        row_colors(&washed_row(Duration::from_millis(75))),
        vec![DRAWN; 90],
        "half way the cells still fade"
    );
    assert_eq!(
        row_colors(&washed_row(Duration::from_millis(150))),
        vec![DRAWN; 90],
        "after 150 ms every cell is drawn"
    );
}

#[test]
fn a_theme_wash_leaves_a_skipped_cell_as_drawn() {
    let mut row = filled_row(90);
    if let Some(cell) = row.cell_mut(Position { x: 40, y: 0 }) {
        cell.set_diff_option(CellDiffOption::Skip);
    }
    let middle = row_colors(&washed(
        vec![OLD; 90].into(),
        row,
        slice(|t| t.screen_wash, 2),
    ));
    assert_eq!(
        middle.get(40),
        Some(&DRAWN),
        "an image placement keeps its drawn colours"
    );
    assert_ne!(
        middle.first(),
        Some(&DRAWN),
        "the cells around it still fade"
    );
}

#[test]
fn a_theme_wash_from_another_screen_size_leaves_the_frame_as_drawn() {
    let resized = washed(
        vec![OLD; 89].into(),
        filled_row(90),
        slice(|t| t.screen_wash, 2),
    );
    assert_eq!(
        row_colors(&resized),
        vec![DRAWN; 90],
        "colours of another size are not laid over the frame"
    );
}
