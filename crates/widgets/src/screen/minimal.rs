use std::time::Duration;

use config::SpeedChip;
use kernel::playlist::RepeatMode;
use raster::unit_fraction;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{
    card::{CardStatus, CardView, card_status, status_label},
    primitive::{
        bar::{BarFill, fill_line},
        chip::{ChipColors, speed_chip_spans, speed_chip_width},
        relative_time::elapsed_of,
        span::{line, text},
        text::truncate,
    },
    theme::{ActiveTheme, FillColors},
};

#[derive(Debug, Clone, Copy)]
pub struct MinimalScreen<'a> {
    pub view: CardView<'a>,
    pub theme: ActiveTheme<'a>,
    pub speed_chip: SpeedChip,
}

impl Widget for &MinimalScreen<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let rows = [
            self.title_line(area.width),
            self.progress_line(area.width),
            self.status_line(area.width),
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

#[must_use]
pub(crate) fn progress_bar_width(
    view: CardView<'_>,
    speed_chip: SpeedChip,
    width: u16,
) -> u16 {
    let duration = view
        .displayed_track
        .and_then(|track| track.duration())
        .unwrap_or(Duration::ZERO);
    let position = view.player.position_at(view.now);
    let time = elapsed_of(position, duration);
    let time_width = u16::try_from(time.chars().count())
        .unwrap_or(u16::MAX)
        .min(width);
    let gap = u16::from(width > time_width);
    let chip_width = speed_chip_width(view.speed, speed_chip);
    width.saturating_sub(time_width + gap + chip_width)
}

impl MinimalScreen<'_> {
    fn title_line(&self, width: u16) -> Line<'static> {
        let status = card_status(self.view.output, self.view.player);
        let color = match status {
            CardStatus::OutputLost => self.theme.secondary_accent(),
            CardStatus::Playing => self.theme.accent(),
            CardStatus::Paused => self.theme.text(),
            CardStatus::Stopped => self.theme.dim(),
        };
        let title = self
            .view
            .displayed_track
            .map_or_else(|| "No track".to_string(), |track| track.song_title());
        let label = format!("{} {title}", status_label(status).glyph);
        line([text(truncate(&label, usize::from(width)).into_owned()).fg(color)])
    }

    fn progress_line(&self, width: u16) -> Line<'static> {
        let accent = self.theme.accent();
        let dim = self.theme.dim();
        let duration = self
            .view
            .displayed_track
            .and_then(|track| track.duration())
            .unwrap_or_default();
        let position = self.view.player.position_at(self.view.now);
        let fraction = if duration.is_zero() {
            0.0
        } else {
            position.as_secs_f64() / duration.as_secs_f64()
        };
        let time = elapsed_of(position, duration);
        let time_width = u16::try_from(time.chars().count())
            .unwrap_or(u16::MAX)
            .min(width);
        let gap = u16::from(width > time_width);
        let chip_width = speed_chip_width(self.view.speed, self.speed_chip);
        let bar_width = progress_bar_width(self.view, self.speed_chip, width);
        let mut spans = fill_line(
            &BarFill::progress(unit_fraction(fraction), usize::from(bar_width)),
            FillColors { accent, dim },
        )
        .spans;
        if bar_width > 0 && gap > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(
            text(truncate(&time, usize::from(time_width)).into_owned())
                .fg(self.theme.text())
                .into(),
        );
        let chip = speed_chip_spans(
            self.view.speed,
            self.speed_chip,
            ChipColors {
                border: dim,
                value: accent,
            },
        );
        if let Some(chip) = chip.filter(|_| chip_width > 0) {
            spans.extend(chip);
        }
        Line::from(spans)
    }

    fn status_line(&self, width: u16) -> Line<'static> {
        let repeat = match self.view.repeat {
            RepeatMode::Off => "Off",
            RepeatMode::All => "All",
            RepeatMode::One => "One",
        };
        let shuffle = if self.view.play_order.is_shuffle() {
            "On"
        } else {
            "Off"
        };
        let status = format!(
            "Vol {}%  Shuf {shuffle}  Rep {repeat}",
            self.view.volume.value()
        );
        Line::from(Span::styled(
            truncate(&status, usize::from(width)).into_owned(),
            Style::default().fg(self.theme.dim()),
        ))
    }
}
