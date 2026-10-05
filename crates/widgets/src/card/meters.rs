use kernel::domain::{appearance::ProgressTime, geometry::Cells};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
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
        bar::{BarFill, HudProgressRow, fill, hud_progress_line},
        chip,
        relative_time::elapsed_of,
        spectrum_meter,
        text::truncate,
    },
};

pub(crate) fn paint(buffer: &mut Buffer, card: &CardWidget<'_>, metrics: &CardMetrics) {
    paint_time_row(buffer, card, metrics);
    paint_progress_text(buffer, card, metrics);
    paint_volume_row(buffer, card, metrics);
}

fn paint_time_row(buffer: &mut Buffer, card: &CardWidget<'_>, metrics: &CardMetrics) {
    let row_width = metrics.row_width;
    let colors = card.theme.colors();
    let dim_color: Color = colors.muted_foreground;

    let current = card.view.displayed_track;
    let elapsed_total = elapsed_of(card.view.position(), card.view.duration());
    let time_row = metrics.time_row;
    let elapsed_width = elapsed_total.chars().count();
    let speed_spans =
        chip::speed_chip_spans(card.view.speed, card.appearance.speed_chip, &colors);
    let speed_width =
        chip::speed_chip_width(card.view.speed, card.appearance.speed_chip).count();
    let left_width = elapsed_width + speed_width;
    let fit = chips::format_chip_fit(
        &FormatChipsInput {
            current,
            visibility: card.appearance.format_chips,
            colors: &colors,
        },
        &ChipBudget {
            available_width: row_width,
            elapsed_width: left_width,
        },
    );
    let elapsed_span: Span<'_> =
        truncated_span(&elapsed_total, fit.elapsed_budget, dim_color);
    let time_line = Line::from_iter(
        std::iter::once(elapsed_span).chain(
            speed_spans
                .into_iter()
                .filter(|_| left_width <= fit.elapsed_budget),
        ),
    );
    Paragraph::new(time_line).render(time_row, buffer);
    if let Some(line) = fit.line {
        paint_format_chips_row(buffer, time_row, line);
    }
}

fn truncated_span(text: &str, budget: usize, color: Color) -> Span<'static> {
    crate::primitive::span::text(truncate(text, budget).into_owned())
        .fg(color)
        .dim()
        .into()
}

fn paint_format_chips_row(buffer: &mut Buffer, time_row: Rect, line: Line<'static>) {
    Paragraph::new(line)
        .alignment(Alignment::Right)
        .render(time_row, buffer);
}

fn paint_progress_text(
    buffer: &mut Buffer,
    card: &CardWidget<'_>,
    metrics: &CardMetrics,
) {
    let row_width = metrics.row_width;
    let bar_colors = (card.theme.progress_fill(), card.theme.progress_groove());

    let fraction = card.view.progress_fraction();
    let progress_row = metrics.progress_row;
    match card.appearance.progress_time {
        ProgressTime::Remaining => Paragraph::new(hud_progress_line(
            &HudProgressRow {
                fraction,
                row_width,
                remaining: card.view.remaining(),
            },
            bar_colors,
            &card.theme.colors(),
        ))
        .render(progress_row, buffer),
        ProgressTime::Elapsed => Paragraph::new(fill(
            &BarFill::progress(fraction, row_width),
            bar_colors.0,
            bar_colors.1,
        ))
        .render(progress_row, buffer),
    }
}

fn paint_volume_row(buffer: &mut Buffer, card: &CardWidget<'_>, metrics: &CardMetrics) {
    let bar_area = metrics.volume_row;
    let colors = card.theme.colors();
    Paragraph::new(fill(
        &BarFill::volume(card.view.volume.ratio(), Cells(bar_area.width)),
        colors.accent,
        colors.bar_groove,
    ))
    .render(bar_area, buffer);

    let spectrum_area = metrics.spectrum_row;
    let spectrum_lines = spectrum_meter::lines(
        &MeterFill {
            size: CanvasSize {
                width: spectrum_area.width,
                height: spectrum_area.height,
            },
            levels: card.view.spectrum,
            max_dots: SPECTRUM_MAX_DOTS,
        },
        |t| card.theme.spectrum_color_at(t),
    );
    Paragraph::new(spectrum_lines).render(spectrum_area, buffer);
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        appearance::AppearanceSettings,
        bounded::Bounded,
        percent::Percent,
        player::Player,
        playhead::Playhead,
        playlist::PlayOrder,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track},
        transport::Output,
    };
    use ratatui::{buffer::Buffer, layout::Rect};

    use crate::{
        card::{
            CardCover,
            CardView,
            CardWidget,
            meters::paint_time_row,
            metrics::card_metrics,
        },
        geometry::{CoverSizing, DEFAULT_CELL_ASPECT},
        primitive::canvas::find_text,
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[test]
    fn the_time_row_reads_the_view_now_not_the_epoch() {
        let theme = noir();
        let track = Arc::new(
            Track::builder()
                .path("/music/song.mp3")
                .duration(Duration::from_secs(245))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        );
        let player = Player::Playing {
            track: Arc::clone(&track),
            playhead: Playhead::anchored(
                Duration::from_secs(10),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        };
        let spectrum: Spectrum = [0.0; SPECTRUM_BANDS];
        let output = Output::Ready;
        let play_order = PlayOrder::default();
        let view = CardView {
            player: &player,
            speed: Speed::default(),
            volume: Percent::clamped(50),
            spectrum: &spectrum,
            repeat: Default::default(),
            play_order: &play_order,
            displayed_track: Some(&track),
            output: &output,
            now: Moment::new(Duration::from_secs(5)),
        };
        let area = Rect::new(0, 0, 60, 12);
        let card_metrics =
            card_metrics(area, DEFAULT_CELL_ASPECT, CoverSizing::default());
        let card = CardWidget {
            view,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            cell_aspect: DEFAULT_CELL_ASPECT,
            cover_sizing: CoverSizing::default(),
            appearance: AppearanceSettings::default(),
            cover_art: &CardCover::Missing,
        };
        let mut buffer = Buffer::empty(area);
        paint_time_row(&mut buffer, &card, &card_metrics);
        assert!(
            find_text(&buffer, "0:15").is_some(),
            "expected the elapsed time to read 0:15, offset by the view's now"
        );
    }
}
