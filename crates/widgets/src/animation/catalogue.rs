use ratatui::{
    buffer::{Buffer, Cell},
    layout::Position,
    style::Color,
};
use tachyonfx::{
    CellFilter,
    ColorSpace,
    Effect as Animation,
    FilterProcessor,
    Interpolation,
    SimpleRng,
    fx,
    fx::ShaderFnContext,
    pattern::{AnyPattern, RadialPattern},
};

use crate::animation::timings::{AnimationTimings, THEME_WASH_GRADIENT_CELLS};

#[must_use]
pub fn modal_in(timings: AnimationTimings) -> Animation {
    modal_reveal(timings.modal_in, modal_pattern(timings), timings)
}

#[must_use]
pub fn modal_out(timings: AnimationTimings) -> Animation {
    modal_reveal(timings.modal_out, modal_pattern(timings), timings)
}

fn modal_pattern(timings: AnimationTimings) -> RadialPattern {
    RadialPattern::center().with_transition_width(timings.modal_transition_width)
}

fn modal_reveal(
    duration: (u32, Interpolation),
    pattern: impl Into<AnyPattern>,
    timings: AnimationTimings,
) -> Animation {
    fx::coalesce(duration)
        .with_rng(SimpleRng::new(timings.scatter_seed))
        .with_pattern(pattern)
}

#[derive(Clone, Copy)]
struct CardSlide {
    background: Color,
    hidden: f32,
}

#[must_use]
pub fn toast_slide_in(background: Color, timings: AnimationTimings) -> Animation {
    fx::effect_fn_buf(
        (),
        timings.toast_slide_in,
        move |_state, context, buffer| {
            let hidden = 1.0 - context.alpha();
            slide_inside_the_card(CardSlide { background, hidden }, &context, buffer);
        },
    )
}

fn slide_inside_the_card(
    slide: CardSlide,
    context: &ShaderFnContext<'_>,
    buffer: &mut Buffer,
) {
    let CardSlide { background, hidden } = slide;
    let card = context.area.intersection(buffer.area);
    let shift = crate::pixels::round::<u16>(f32::from(card.width) * hidden);
    if shift == 0 {
        return;
    }
    let allowed = context.filter().map(FilterProcessor::validator);
    let mut vacated = Cell::EMPTY;
    vacated.set_bg(background);
    for row in card.y..card.bottom() {
        for step in 0..card.width {
            let landing = Position {
                x: card.right().saturating_sub(step).saturating_sub(1),
                y: row,
            };
            let mut arriving = vacated.clone();
            if let Some(lifted) =
                landing.x.checked_sub(shift).filter(|from| *from >= card.x)
                && let Some(cell) = buffer.cell((lifted, row))
            {
                arriving = cell.clone();
            }
            if let Some(cell) = buffer.cell_mut(landing)
                && allowed
                    .as_ref()
                    .is_none_or(|guard| guard.is_valid(landing, cell))
            {
                *cell = arriving;
            }
        }
    }
}

#[must_use]
pub fn chip_pulse(target: Color, timings: AnimationTimings) -> Animation {
    fx::ping_pong(fx::fade_to_fg(target, timings.chip_pulse_half))
}

#[must_use]
pub fn row_flash(accent: Color, timings: AnimationTimings) -> Animation {
    fx::fade_from_fg(accent, timings.row_flash)
}

#[must_use]
pub fn favorite_pulse(accent: Color, timings: AnimationTimings) -> Animation {
    fx::ping_pong(fx::fade_to_fg(accent, timings.favorite_pulse_half))
}

#[derive(Debug, Clone, Copy)]
pub struct VolumeShades {
    pub fill: Color,
    pub lifted: Color,
}

#[must_use]
pub fn volume_pulse(
    shades: VolumeShades,
    guard: CellFilter,
    timings: AnimationTimings,
) -> Animation {
    fx::fade_from_fg(shades.lifted, timings.volume_pulse).with_filter(
        CellFilter::AllOf(vec![guard, CellFilter::FgColor(shades.fill)]),
    )
}

#[must_use]
pub fn wash_reveal(progress: f32, column: u16, width: u16) -> f32 {
    let gradient = f32::from(THEME_WASH_GRADIENT_CELLS);
    let window = f32::from(width) + gradient;
    ((progress * window - f32::from(column)) / gradient).clamp(0.0, 1.0)
}

#[must_use]
pub fn screen_wash(from: Color, timings: AnimationTimings) -> Animation {
    fx::effect_fn_buf((), timings.screen_wash, move |_state, context, buffer| {
        wash_buffer(from, &context, buffer);
    })
}

fn wash_buffer(from: Color, context: &ShaderFnContext<'_>, buffer: &mut Buffer) {
    let area = context.area.intersection(buffer.area);
    let alpha = context.alpha();
    let allowed = context.filter().map(FilterProcessor::validator);
    for row in area.y..area.bottom() {
        for x in area.x..area.right() {
            let position = Position { x, y: row };
            let Some(cell) = buffer.cell_mut(position) else {
                continue;
            };
            if allowed
                .as_ref()
                .is_some_and(|guard| !guard.is_valid(position, cell))
            {
                continue;
            }
            let reveal = wash_reveal(alpha, x - area.x, area.width);
            let fg = ColorSpace::Rgb.lerp(&from, &cell.fg, reveal);
            let bg = ColorSpace::Rgb.lerp(&from, &cell.bg, reveal);
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
    }
}

#[must_use]
pub fn delete_burst(
    background: Color,
    guard: CellFilter,
    timings: AnimationTimings,
) -> Animation {
    scatter_burst(background, guard, timings)
}

#[must_use]
pub fn toast_burst(
    background: Color,
    guard: CellFilter,
    timings: AnimationTimings,
) -> Animation {
    scatter_burst(background, guard, timings)
}

fn scatter_burst(
    background: Color,
    guard: CellFilter,
    timings: AnimationTimings,
) -> Animation {
    fx::parallel(&[
        fx::explode(
            timings.delete_force,
            timings.delete_force_variance,
            timings.delete_burst,
        )
        .with_rng(SimpleRng::new(timings.scatter_seed))
        .with_filter(guard.clone())
        .reversed(),
        fx::paint_bg(background, timings.delete_burst).with_filter(CellFilter::AllOf(
            vec![CellFilter::BgColor(Color::Black), guard],
        )),
    ])
}
