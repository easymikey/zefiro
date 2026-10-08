use kernel::domain::{appearance::ProgressTime, geometry::Cells};
use ratatui::{
    buffer::Buffer,
    layout::Alignment,
    style::Color,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    braille::{CanvasSize, MeterFill},
    card::{
        CardWidget,
        chips::{self, ChipBudget, FormatChipsInput},
        metrics::{CardMetrics, SPECTRUM_MAX_DOTS},
    },
    primitive::{
        bar::{BarFill, HudProgress, hud_progress_line},
        canvas::Canvas,
        chip,
        span,
        spectrum_meter,
        time_text::elapsed_text,
        truncate::truncate_owned,
    },
};

pub(crate) fn paint(
    buffer: &mut Buffer,
    card_widget: &CardWidget<'_>,
    metrics: &CardMetrics,
) {
    paint_time_row(buffer, card_widget, metrics);
    paint_progress_row(buffer, card_widget, metrics);
    paint_volume_row(buffer, card_widget, metrics);
}

fn paint_time_row(
    buffer: &mut Buffer,
    card_widget: &CardWidget<'_>,
    metrics: &CardMetrics,
) {
    let row_width = metrics.row_width;
    let colors = card_widget.active_theme.colors();
    let dim_color: Color = colors.muted_foreground;

    let displayed_track = card_widget.view.displayed_track;
    let elapsed_total =
        elapsed_text(card_widget.view.position(), card_widget.view.duration());
    let time_row = metrics.time_row;
    let elapsed_width = elapsed_total.chars().count();
    let speed_spans = chip::speed_chip_spans(
        card_widget.view.speed,
        card_widget.appearance_settings.speed_chip,
        &colors,
    );
    let speed_width = span::width(&speed_spans);
    let left_width = elapsed_width + speed_width;
    let fit = chips::FormatChipFit::new(
        &FormatChipsInput {
            displayed_track,
            format_chips: card_widget.appearance_settings.format_chips,
            colors: &colors,
        },
        &ChipBudget {
            available_width: row_width,
            elapsed_width: left_width,
        },
    );
    let elapsed_span: Span<'_> =
        span::text(truncate_owned(elapsed_total, fit.elapsed_budget))
            .fg(dim_color)
            .dim()
            .into();
    let speed_spans = if left_width <= fit.elapsed_budget {
        speed_spans
    } else {
        Vec::new()
    };
    let time_line = Line::from_iter(std::iter::once(elapsed_span).chain(speed_spans));
    Paragraph::new(time_line).render(time_row, buffer);
    if let Some(line) = fit.line {
        Paragraph::new(line)
            .alignment(Alignment::Right)
            .render(time_row, buffer);
    }
}

fn paint_progress_row(
    buffer: &mut Buffer,
    card_widget: &CardWidget<'_>,
    metrics: &CardMetrics,
) {
    let bar_width = card_widget.progress_bar_width;
    let fill = card_widget.active_theme.progress_fill();
    let groove = card_widget.active_theme.progress_groove();
    let fraction = card_widget.view.progress_fraction();
    let progress_row = metrics.progress_row;
    match card_widget.appearance_settings.progress_time {
        ProgressTime::Remaining => Paragraph::new(hud_progress_line(
            &HudProgress {
                fraction,
                row_width: Cells(progress_row.width),
                bar_width,
                remaining_label: card_widget.remaining_label,
                fill,
                groove,
            },
            &card_widget.active_theme.colors(),
        ))
        .render(progress_row, buffer),
        ProgressTime::Elapsed => {
            Paragraph::new(BarFill::progress(fraction, bar_width).line(fill, groove))
                .render(progress_row, buffer);
        }
    }
}

fn paint_volume_row(
    buffer: &mut Buffer,
    card_widget: &CardWidget<'_>,
    metrics: &CardMetrics,
) {
    let bar_area = metrics.volume_row;
    let colors = card_widget.active_theme.colors();
    Paragraph::new(
        BarFill::volume(card_widget.view.volume.ratio(), Cells(bar_area.width))
            .line(colors.accent, colors.bar_groove),
    )
    .render(bar_area, buffer);

    let spectrum_area = metrics.spectrum_row;
    spectrum_meter::paint(
        &MeterFill {
            size: CanvasSize {
                width: spectrum_area.width,
                height: spectrum_area.height,
            },
            levels: card_widget.view.spectrum,
            max_dots: SPECTRUM_MAX_DOTS,
        },
        Canvas {
            area: spectrum_area,
            buffer,
        },
        |fraction| card_widget.active_theme.spectrum_color_at(fraction),
    );
}
