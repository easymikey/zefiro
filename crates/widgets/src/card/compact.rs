use kernel::domain::{appearance::SpeedChip, geometry::Cells};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::{
    card::{
        CardView,
        card_frame,
        headings::{card_status, status_color, status_label},
    },
    primitive::{
        bar::{BarFill, fill},
        chip::{speed_chip_spans, speed_chip_width},
        relative_time::elapsed_of,
        span::{line, text},
        text::truncate,
    },
    theme::active_theme::ActiveTheme,
};

const PADDING: u16 = 1;
const BORDER_WIDTH: u16 = 2;
const TITLE_ROWS: u16 = 2;
const PROGRESS_ROWS: u16 = 1;
const STATUS_ROWS: u16 = 1;
const VOLUME_BAR_WIDTH: u16 = 16;

#[must_use]
pub(crate) fn compact_height() -> u16 {
    BORDER_WIDTH + TITLE_ROWS + PROGRESS_ROWS + STATUS_ROWS
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompactCardWidget<'a> {
    view: CardView<'a>,
    theme: ActiveTheme<'a>,
    speed_chip: SpeedChip,
}

impl<'a> CompactCardWidget<'a> {
    #[must_use]
    pub(crate) fn new(view: CardView<'a>, theme: ActiveTheme<'a>) -> Self {
        Self {
            view,
            theme,
            speed_chip: SpeedChip::default(),
        }
    }

    #[must_use]
    pub(crate) fn speed_chip(mut self, speed_chip: SpeedChip) -> Self {
        self.speed_chip = speed_chip;
        self
    }
}

struct CompactParts<'a> {
    view: CardView<'a>,
    theme: ActiveTheme<'a>,
    speed_chip: SpeedChip,
    inner: Rect,
    status_row: StatusRowGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusRowGeometry {
    row: Cells,
    status_width: Cells,
    volume_width: Cells,
}

fn status_row_geometry(inner: Rect) -> StatusRowGeometry {
    let row_width = inner.width;
    let volume_width = VOLUME_BAR_WIDTH.min(row_width / 2);
    StatusRowGeometry {
        row: Cells(inner.y + TITLE_ROWS + PROGRESS_ROWS),
        status_width: Cells(row_width.saturating_sub(volume_width + 1)),
        volume_width: Cells(volume_width),
    }
}

fn content_area(area: Rect) -> Rect {
    let inner = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .inner(area);
    Rect {
        x: inner.x + PADDING,
        y: inner.y,
        width: inner.width.saturating_sub(PADDING * 2),
        height: inner.height,
    }
}

#[must_use]
pub(crate) fn progress_bar_width(area: Rect) -> u16 {
    content_area(area).width
}

impl Widget for &CompactCardWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.theme;
        let frame_color: Color = theme.colors().muted_foreground;

        let inner = content_area(area);
        card_frame(frame_color).render(area, buffer);

        let context = CompactParts {
            view: self.view,
            theme,
            speed_chip: self.speed_chip,
            inner,
            status_row: status_row_geometry(inner),
        };
        paint_header_row(buffer, &context);
        paint_progress_row(buffer, &context);
        paint_status_row(buffer, &context);
        paint_meter_row(buffer, &context);
    }
}

fn paint_header_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let row_width = inner.width;
    let colors = context.theme.colors();

    let title = context.view.title();
    let artist = context.view.artist();

    let title_row = clamp(Rect {
        x: inner.x,
        y: inner.y,
        width: row_width,
        height: 1,
    });
    Paragraph::new(line([text(truncate(title, usize::from(row_width)))
        .fg(colors.text)
        .bold()]))
    .render(title_row, buffer);

    let artist_row = clamp(Rect {
        x: inner.x,
        y: inner.y + 1,
        width: row_width,
        height: 1,
    });
    Paragraph::new(line([
        text(truncate(artist, usize::from(row_width))).fg(colors.muted_foreground)
    ]))
    .render(artist_row, buffer);
}

fn paint_progress_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let row_width = inner.width;
    let progress_y = inner.y + TITLE_ROWS;
    let progress_row = clamp(Rect {
        x: inner.x,
        y: progress_y,
        width: row_width,
        height: 1,
    });
    Paragraph::new(fill(
        &BarFill::progress(context.view.progress_fraction(), Cells(row_width)),
        context.theme.progress_fill(),
        context.theme.progress_groove(),
    ))
    .render(progress_row, buffer);
}

fn paint_status_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let StatusRowGeometry {
        row, status_width, ..
    } = context.status_row;
    let view = context.view;
    let status = card_status(view.output, view.player);
    let status_color = status_color(&context.theme, status);
    let label = status_label(status);

    let status_base = format!("{} {}", label.glyph, label.word);
    let elapsed_total = elapsed_of(view.position(), view.duration());
    let status_text = format!("{status_base}  {elapsed_total}");
    let status_row = clamp(Rect {
        x: inner.x,
        y: row.0,
        width: status_width.0,
        height: 1,
    });
    let indicator_spans =
        speed_chip_spans(view.speed, context.speed_chip, &context.theme.colors());
    let indicator_width = speed_chip_width(view.speed, context.speed_chip).count();
    let fits = status_text.chars().count() + indicator_width <= status_width.count();
    let status_spans: Vec<_> = std::iter::once(
        text(truncate(&status_text, status_width.count()))
            .fg(status_color)
            .into(),
    )
    .chain(indicator_spans.into_iter().filter(|_| fits))
    .collect();
    Paragraph::new(Line::from(status_spans)).render(status_row, buffer);
}

fn paint_meter_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let row_width = inner.width;
    let StatusRowGeometry {
        row, volume_width, ..
    } = context.status_row;

    let bar_area = clamp(Rect {
        x: inner.x + row_width.saturating_sub(volume_width.0),
        y: row.0,
        width: volume_width.0,
        height: 1,
    });
    Paragraph::new(fill(
        &BarFill::volume(context.view.volume.ratio(), Cells(bar_area.width)),
        context.theme.colors().accent,
        context.theme.colors().bar_groove,
    ))
    .render(bar_area, buffer);
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        appearance::{ProgressBar, Rgb, SpeedChip},
        bounded::Bounded,
        percent::Percent,
        player::Player,
        playhead::Playhead,
        playlist::PlayOrder,
        speed::Speed,
        time::Moment,
        transport::Output,
    };

    use crate::{
        card::{
            CardView,
            compact::{CompactCardWidget, compact_height},
        },
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered, track},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[test]
    fn the_compact_card_shows_title_progress_and_status() {
        let theme = noir();
        let track = track("Moon River");
        let player = Player::Playing {
            track: Arc::clone(&track),
            playhead: Playhead::anchored(
                Duration::from_secs(30),
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
            now: Moment::default(),
        };
        let widget = CompactCardWidget::new(
            view,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .speed_chip(SpeedChip::Always);
        let text = rendered(40, compact_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(text.contains("Moon River"), "got {text:?}");
        assert!(text.contains("Playing"), "got {text:?}");
    }

    #[test]
    fn no_track_shows_a_placeholder_title_and_a_stopped_status() {
        let theme = noir();
        let player = Player::Stopped;
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
            displayed_track: None,
            output: &output,
            now: Moment::default(),
        };
        let widget = CompactCardWidget::new(
            view,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .speed_chip(SpeedChip::Always);
        let text = rendered(40, compact_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(text.contains("No track"), "got {text:?}");
        assert!(text.contains("Stopped"), "got {text:?}");
    }

    #[test]
    fn compact_volume_bar_uses_the_bar_groove() {
        let theme = noir();
        let player = Player::Stopped;
        let spectrum: Spectrum = [0.0; SPECTRUM_BANDS];
        let output = Output::Ready;
        let play_order = PlayOrder::default();
        let view = CardView {
            player: &player,
            speed: Speed::default(),
            volume: Percent::clamped(0),
            spectrum: &spectrum,
            repeat: Default::default(),
            play_order: &play_order,
            displayed_track: None,
            output: &output,
            now: Moment::default(),
        };
        let bar = ProgressBar {
            groove: Some(Rgb([0, 255, 0])),
            ..ProgressBar::default()
        };
        let widget = CompactCardWidget::new(
            view,
            ActiveTheme::new(&theme, ColorDepth::TrueColor).with_progress(bar),
        )
        .speed_chip(SpeedChip::Always);
        let backend = rendered(40, compact_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        });
        let colors = ActiveTheme::new(&theme, ColorDepth::TrueColor).colors();
        let cell = &backend.buffer()[(22, 4)];
        assert_eq!(cell.fg, colors.bar_groove);
    }
}
