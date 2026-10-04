use std::time::Duration;

use kernel::{Cue, PlaybackChange};
use ratatui::{
    buffer::Buffer,
    style::{Color, Style},
};
use widgets::AnimationStage;

use crate::unit::{
    animation_stage::step_over,
    support::{
        ACCENT,
        AREA,
        VOLUME_LABEL,
        chip_backdrop,
        pane_backdrop,
        slice,
        volume_bar_frame,
        volume_fill,
        volume_lifted,
    },
};

const FILLED_CELLS: u16 = 3;

fn volume_marks() -> [Duration; 4] {
    let quarter = slice(|timings| timings.volume_pulse, 4);
    [
        Duration::ZERO,
        quarter,
        quarter,
        slice(|timings| timings.volume_pulse, 2),
    ]
}

fn channels(color: Color) -> [u8; 3] {
    if let Color::Rgb(red, green, blue) = color {
        [red, green, blue]
    } else {
        [0, 0, 0]
    }
}

fn channel_gap(left: Color, right: Color) -> u8 {
    channels(left)
        .into_iter()
        .zip(channels(right))
        .map(|(one, other)| one.abs_diff(other))
        .fold(0, u8::max)
}

fn volume_cells(frame: &Buffer) -> (Vec<Color>, Vec<Color>) {
    let split = VOLUME_LABEL.x.saturating_add(FILLED_CELLS);
    let foreground = |column: u16| {
        frame
            .cell((column, VOLUME_LABEL.y))
            .map_or(Color::Reset, |cell| cell.fg)
    };
    (
        (VOLUME_LABEL.x..split).map(foreground).collect(),
        (split..VOLUME_LABEL.right()).map(foreground).collect(),
    )
}

fn widest_gap(now: &[Color], base: &[Color]) -> u8 {
    now.iter()
        .zip(base.iter())
        .map(|(one, other)| channel_gap(*one, *other))
        .fold(0, u8::max)
}

fn volume_storyboard(stage: &mut AnimationStage) -> String {
    let painted = volume_bar_frame();
    let (base_fill, base_groove) = volume_cells(&painted);
    let bound = channel_gap(volume_fill(), volume_lifted());
    let mut ended = base_fill.clone();
    let mut clock = Duration::ZERO;
    let mut out = String::new();
    for mark in volume_marks() {
        clock = clock.saturating_add(mark);
        let elapsed = clock.as_millis();
        let frame = step_over(stage, volume_bar_frame, mark);
        let (fill, groove) = volume_cells(&frame);
        assert_eq!(groove, base_groove, "the groove moved at t={elapsed}ms");
        let gap = widest_gap(&fill, &base_fill);
        assert!(
            gap <= bound,
            "the fill shifted {gap} at t={elapsed}ms, over {bound}"
        );
        out.push_str(&format!(
            "t={elapsed}ms gap={gap} fill={fill:?} groove={groove:?}\n"
        ));
        ended = fill;
    }
    assert_eq!(ended, base_fill, "the pulse must land back on the fill");
    out
}

#[test]
fn volume_pulse_storyboard() {
    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::VolumeChanged], &pane_backdrop());

    insta::assert_snapshot!(volume_storyboard(&mut stage));
}

#[test]
fn the_chip_pulse_moves_a_chip_the_card_already_drew_in_accent() {
    let mut stage = AnimationStage::default();
    stage.play(
        vec![Cue::PlaybackChanged(PlaybackChange::Play)],
        &chip_backdrop(),
    );

    let mut accent_chip = Buffer::empty(AREA);
    accent_chip.set_string(0, 0, "Playing ", Style::default().fg(ACCENT));
    let before = accent_chip.clone();
    stage.advance(&mut accent_chip, slice(|t| t.chip_pulse_half, 2));

    assert_ne!(
        accent_chip, before,
        "an accent-on-window chip must still visibly move"
    );
}

#[test]
fn an_enabled_animation_wants_frames_and_advances_over_ticks() {
    let pulse_tick = slice(|t| t.volume_pulse, 8);

    let mut stage = AnimationStage::default();
    stage.play(vec![Cue::VolumeChanged], &pane_backdrop());
    assert!(stage.is_running(), "an enabled animation asks for frames");

    let mut seen: Vec<Buffer> = Vec::new();
    for tick in 0..4 {
        assert!(
            stage.is_running(),
            "the animation must still want a frame at tick {tick} — 4 ticks of {pulse_tick:?} is well inside the fade"
        );
        let frame = step_over(&mut stage, volume_bar_frame, pulse_tick);
        assert_ne!(frame, volume_bar_frame(), "tick {tick} must move the frame");
        assert!(
            !seen.contains(&frame),
            "tick {tick} must advance the animation, not repaint the previous step"
        );
        seen.push(frame);
    }

    let mut guard = 0;
    while stage.is_running() {
        step_over(&mut stage, volume_bar_frame, pulse_tick);
        guard += 1;
        assert!(guard < 64, "the animation must end");
    }
}
