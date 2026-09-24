use std::time::Duration;

use config::ProgressStyle;
use raster::unit_fraction;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::Color,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    braille::{BrailleBuffers, CanvasSize, MeterFill, RowRounding},
    card::{
        CardContext,
        CardMetrics,
        CardView,
        chips::{self, ChipBudget, FormatChipContent},
    },
    primitive::{
        bar::{
            FillSpec,
            HudProgressColors,
            HudProgressRow,
            fill_line,
            hud_progress_line,
        },
        chip::{self, ChipColors},
        relative_time::elapsed_of,
        spectrum_meter,
        text::truncate,
    },
};

#[cfg(test)]
#[must_use]
pub(crate) fn volume_cell_size() -> (u16, u16) {
    let layout = crate::card::CardLayout::default();
    (layout.volume_bar_width, layout.volume_height)
}

fn remaining(view: CardView<'_>) -> Duration {
    let duration = view
        .displayed_track
        .and_then(|track| track.duration())
        .unwrap_or_default();
    duration.saturating_sub(view.player.position())
}

pub(crate) fn paint(
    buffer: &mut Buffer,
    context: &CardContext<'_>,
    spectrum_buffers: &mut BrailleBuffers,
) {
    paint_time_row(buffer, context);
    paint_progress_text(buffer, context);
    paint_volume_row(buffer, context, spectrum_buffers);
}

fn paint_time_row(buffer: &mut Buffer, context: &CardContext<'_>) {
    let metrics: &CardMetrics = context.metrics;
    let row_width = metrics.row_width;
    let dim_color: Color = context.theme.dim();
    let accent_color: Color = context.theme.accent();
    let text_color: Color = context.theme.text();

    let current = context.view.displayed_track;
    let duration = current
        .and_then(|track| track.duration())
        .unwrap_or_default();
    let position = context.view.player.position();
    let elapsed_total = elapsed_of(position, duration);
    let time_row = metrics.time_row;
    let elapsed_width = elapsed_total.chars().count();
    let speed_spans = chip::speed_chip_spans(
        context.view.speed,
        context.appearance.speed_chip,
        ChipColors {
            border: dim_color,
            value: accent_color,
        },
    );
    let speed_width = usize::from(chip::speed_chip_width(
        context.view.speed,
        context.appearance.speed_chip,
    ));
    let left_width = elapsed_width + speed_width;
    let fit = chips::format_chip_fit(
        &FormatChipContent {
            current,
            visibility: context.appearance.format_chips,
            colors: ChipColors {
                border: dim_color,
                value: text_color,
            },
        },
        &ChipBudget {
            available_width: row_width,
            elapsed_width: left_width,
        },
    );
    let elapsed_span: Span<'_> = text_of(&elapsed_total, fit.elapsed_budget, dim_color);
    let time_line = Line::from_iter(
        std::iter::once(elapsed_span).chain(
            speed_spans
                .filter(|_| left_width <= fit.elapsed_budget)
                .into_iter()
                .flatten(),
        ),
    );
    Paragraph::new(time_line).render(time_row, buffer);
    paint_format_chips_row(buffer, time_row, fit.line);
}

fn text_of(value: &str, budget: usize, color: Color) -> Span<'static> {
    crate::primitive::span::text(truncate(value, budget).into_owned())
        .fg(color)
        .dim()
        .into()
}

fn paint_format_chips_row(
    buffer: &mut Buffer,
    time_row: Rect,
    line: Option<Line<'static>>,
) {
    if let Some(line) = line {
        Paragraph::new(line)
            .alignment(Alignment::Right)
            .render(time_row, buffer);
    }
}

fn chip_colors(context: &CardContext<'_>) -> ChipColors {
    ChipColors {
        border: context.theme.dim(),
        value: context.theme.text(),
    }
}

fn paint_progress_text(buffer: &mut Buffer, context: &CardContext<'_>) {
    let metrics: &CardMetrics = context.metrics;
    let row_width = metrics.row_width;
    let bar_colors = context.theme.progress_colors();

    let current = context.view.displayed_track;
    let duration = current
        .and_then(|track| track.duration())
        .unwrap_or_default();
    let position = context.view.player.position();
    let fraction = if duration.is_zero() {
        0.0
    } else {
        position.as_secs_f64() / duration.as_secs_f64()
    };
    let progress_row = metrics.progress_row;
    match context.appearance.progress_remaining {
        ProgressStyle::Remaining => Paragraph::new(hud_progress_line(
            &HudProgressRow {
                frac: unit_fraction(fraction),
                row_width: usize::from(row_width),
                remaining: remaining(context.view),
            },
            &HudProgressColors {
                bar: bar_colors,
                chip: chip_colors(context),
            },
        ))
        .render(progress_row, buffer),
        ProgressStyle::Elapsed => Paragraph::new(fill_line(
            &FillSpec::progress(unit_fraction(fraction), usize::from(row_width)),
            bar_colors,
        ))
        .render(progress_row, buffer),
    }
}

fn paint_volume_row(
    buffer: &mut Buffer,
    context: &CardContext<'_>,
    spectrum_buffers: &mut BrailleBuffers,
) {
    let metrics: &CardMetrics = context.metrics;
    let layout = context.layout;

    let bar_area = metrics.volume_row;
    Paragraph::new(fill_line(
        &FillSpec::volume(context.view.volume.ratio(), usize::from(bar_area.width)),
        context.theme.volume_colors(),
    ))
    .render(bar_area, buffer);

    let spectrum_area = metrics.spectrum_row;
    let spectrum_lines = spectrum_meter::lines(
        &MeterFill {
            size: CanvasSize {
                width: spectrum_area.width,
                height: spectrum_area.height,
            },
            levels: context.view.spectrum,
            max_dots: layout.spectrum_max_dots,
            rounding: RowRounding::default(),
        },
        |t| context.theme.spectrum_color_at(t),
        spectrum_buffers,
    );
    Paragraph::new(spectrum_lines).render(spectrum_area, buffer);
}
