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
    SimpleRng,
    fx,
    fx::ShaderFnContext,
    pattern::RadialPattern,
};

use crate::animation::timings::{THEME_WASH_GRADIENT_CELLS, TIMINGS};

#[must_use]
pub fn modal_reveal() -> Animation {
    fx::coalesce(TIMINGS.modal_reveal)
        .with_rng(SimpleRng::new(TIMINGS.scatter_seed))
        .with_pattern(
            RadialPattern::center()
                .with_transition_width(TIMINGS.modal_transition_width),
        )
}

#[derive(Clone, Copy)]
struct CardSlide {
    background: Color,
    hidden: f32,
}

#[must_use]
pub fn toast_slide_in(background: Color) -> Animation {
    fx::effect_fn_buf(
        (),
        TIMINGS.toast_slide_in,
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
    let shift = crate::pixels::numeric::round::<u16>(f32::from(card.width) * hidden);
    if shift == 0 {
        return;
    }
    let allowed = context.filter().map(FilterProcessor::validator);
    let vacated = Cell::default().set_bg(background).clone();
    for row in card.y..card.bottom() {
        for step in 0..card.width {
            let landing = Position {
                x: card.right().saturating_sub(step).saturating_sub(1),
                y: row,
            };
            let arriving = landing
                .x
                .checked_sub(shift)
                .filter(|from| *from >= card.x)
                .and_then(|lifted| buffer.cell((lifted, row)))
                .unwrap_or(&vacated)
                .clone();
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
pub fn chip_pulse(target: Color) -> Animation {
    fx::ping_pong(fx::fade_to_fg(target, TIMINGS.chip_pulse_half))
}

#[must_use]
pub fn row_flash(accent: Color) -> Animation {
    fx::fade_from_fg(accent, TIMINGS.row_flash)
}

#[must_use]
pub fn volume_pulse(fill: Color, lifted: Color, guard: CellFilter) -> Animation {
    fx::fade_from_fg(lifted, TIMINGS.volume_pulse)
        .with_filter(CellFilter::AllOf(vec![guard, CellFilter::FgColor(fill)]))
}

#[must_use]
pub fn wash_reveal(progress: f32, column: u16, width: u16) -> f32 {
    let gradient = f32::from(THEME_WASH_GRADIENT_CELLS);
    let window = f32::from(width) + gradient;
    ((progress * window - f32::from(column)) / gradient).clamp(0.0, 1.0)
}

#[must_use]
pub fn screen_wash(from: Color) -> Animation {
    fx::effect_fn_buf((), TIMINGS.screen_wash, move |_state, context, buffer| {
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
pub fn scatter_burst(background: Color, guard: CellFilter) -> Animation {
    fx::parallel(&[
        fx::explode(
            TIMINGS.delete_force,
            TIMINGS.delete_force_variance,
            TIMINGS.delete_burst,
        )
        .with_rng(SimpleRng::new(TIMINGS.scatter_seed))
        .with_filter(guard.clone())
        .reversed(),
        fx::paint_bg(background, TIMINGS.delete_burst).with_filter(CellFilter::AllOf(
            vec![CellFilter::BgColor(Color::Black), guard],
        )),
    ])
}
