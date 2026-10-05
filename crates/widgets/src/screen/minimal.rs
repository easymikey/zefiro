use kernel::domain::{appearance::SpeedChip, geometry::Cells};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    card::{
        CardView,
        headings::{card_status, status_color, status_label},
    },
    pixels::numeric::small_count_u16,
    primitive::{
        bar::{BarFill, fill},
        chip::{speed_chip_spans, speed_chip_width},
        relative_time::elapsed_of,
        span::{line, text},
        text::truncate,
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MinimalScreenWidget<'a> {
    pub(crate) view: CardView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) speed_chip: SpeedChip,
}

impl Widget for &MinimalScreenWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let rows = [
            self.title_line(Cells(area.width)),
            self.progress_line(Cells(area.width)),
            self.status_line(Cells(area.width)),
        ];
        for (offset, line) in (0..area.height).zip(rows) {
            Paragraph::new(line).render(
                Rect {
                    y: area.y + offset,
                    height: 1,
                    ..area
                },
                buffer,
            );
        }
    }
}

struct MinimalProgress {
    elapsed_text: String,
    bar_width: Cells,
    gap: u16,
}

impl MinimalProgress {
    fn new(view: CardView<'_>, speed_chip: SpeedChip, width: Cells) -> Self {
        let time = elapsed_of(view.position(), view.duration());
        let time_width = small_count_u16(time.chars().count()).min(width.0);
        let gap = u16::from(width.0 > time_width);
        let chip_width = speed_chip_width(view.speed, speed_chip);
        Self {
            elapsed_text: truncate(&time, usize::from(time_width)).into_owned(),
            bar_width: Cells(width.0.saturating_sub(time_width + gap + chip_width.0)),
            gap,
        }
    }
}

#[must_use]
pub(crate) fn progress_bar_width(
    view: CardView<'_>,
    speed_chip: SpeedChip,
    width: Cells,
) -> Cells {
    MinimalProgress::new(view, speed_chip, width).bar_width
}

impl MinimalScreenWidget<'_> {
    fn title_line(&self, width: Cells) -> Line<'static> {
        let status = card_status(self.view.output, self.view.player);
        let color = status_color(&self.theme, status);
        let title = self.view.title();
        let label = format!("{} {title}", status_label(status).glyph);
        line([text(truncate(&label, width.count()).into_owned()).fg(color)])
    }

    fn progress_line(&self, width: Cells) -> Line<'static> {
        let colors = self.theme.colors();
        let accent = colors.accent;
        let dim = colors.muted_foreground;
        let minimal_progress = MinimalProgress::new(self.view, self.speed_chip, width);
        let chip_width = speed_chip_width(self.view.speed, self.speed_chip);
        let bar_width = minimal_progress.bar_width;
        let mut spans = fill(
            &BarFill::progress(self.view.progress_fraction(), bar_width),
            accent,
            dim,
        )
        .spans;
        if bar_width > Cells(0) && minimal_progress.gap > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(text(minimal_progress.elapsed_text).fg(colors.text).into());
        let chip = speed_chip_spans(self.view.speed, self.speed_chip, &colors);
        if chip_width > Cells(0) {
            spans.extend(chip);
        }
        Line::from(spans)
    }

    fn status_line(&self, width: Cells) -> Line<'static> {
        let repeat = <&str>::from(self.view.repeat);
        let shuffle = if self.view.play_order.is_shuffle() {
            "On"
        } else {
            "Off"
        };
        let status = format!(
            "Vol {}%  Shuf {shuffle}  Rep {repeat}",
            self.view.volume.get()
        );
        Line::from(Span::styled(
            truncate(&status, width.count()).into_owned(),
            Style::default().fg(self.theme.colors().muted_foreground),
        ))
    }
}
