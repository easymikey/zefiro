use kernel::domain::appearance::SpeedChip;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::{
    card::{
        CardView,
        headings::{CardStatus, card_status, status_label},
    },
    pixels::unit_fraction,
    primitive::{
        bar::{BarFill, fill},
        chip::{ChipStyle, speed_chip_spans, speed_chip_width},
        relative_time::elapsed_of,
        span::{line, text},
        text::truncate,
    },
    theme::{ActiveTheme, BarStyle, Role},
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
pub(crate) struct CompactCard<'a> {
    pub(crate) view: CardView<'a>,
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) speed_chip: SpeedChip,
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
    row_y: u16,
    status_width: u16,
    volume_width: u16,
}

fn status_row_geometry(inner: Rect) -> StatusRowGeometry {
    let row_width = inner.width;
    let volume_width = VOLUME_BAR_WIDTH.min(row_width / 2);
    StatusRowGeometry {
        row_y: inner.y + TITLE_ROWS + PROGRESS_ROWS,
        status_width: row_width.saturating_sub(volume_width + 1),
        volume_width,
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

impl Widget for &CompactCard<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.theme;
        let frame_color: Color = theme.role(Role::Frame);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(frame_color))
            .title(" Sifr ")
            .title_style(Style::default().fg(frame_color));
        let inner = content_area(area);
        block.render(area, buffer);

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
    let text_color: Color = context.theme.role(Role::Text);
    let dim_color: Color = context.theme.role(Role::Dim);

    let current = context.view.displayed_track;
    let title =
        current.map_or_else(|| "No track".to_string(), |track| track.song_title());
    let artist = current
        .and_then(|track| track.tags().artist.clone())
        .unwrap_or_default();

    let title_row = clamp(Rect {
        x: inner.x,
        y: inner.y,
        width: row_width,
        height: 1,
    });
    Paragraph::new(line([text(truncate(&title, usize::from(row_width)))
        .fg(text_color)
        .bold()]))
    .render(title_row, buffer);

    let artist_row = clamp(Rect {
        x: inner.x,
        y: inner.y + 1,
        width: row_width,
        height: 1,
    });
    Paragraph::new(line([
        text(truncate(&artist, usize::from(row_width))).fg(dim_color)
    ]))
    .render(artist_row, buffer);
}

fn paint_progress_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let row_width = inner.width;
    let accent_color: Color = context.theme.role(Role::Accent);
    let dim_color: Color = context.theme.role(Role::Dim);

    let duration = context.view.duration();
    let fraction = if duration.is_zero() {
        0.0
    } else {
        context.view.position().as_secs_f64() / duration.as_secs_f64()
    };

    let progress_y = inner.y + TITLE_ROWS;
    let progress_row = clamp(Rect {
        x: inner.x,
        y: progress_y,
        width: row_width,
        height: 1,
    });
    Paragraph::new(fill(
        &BarFill::progress(unit_fraction(fraction), usize::from(row_width)),
        BarStyle {
            fill: accent_color,
            track: dim_color,
        },
    ))
    .render(progress_row, buffer);
}

fn paint_status_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let StatusRowGeometry {
        row_y,
        status_width,
        ..
    } = context.status_row;
    let view = context.view;
    let text_color: Color = context.theme.role(Role::Text);
    let dim_color: Color = context.theme.role(Role::Dim);
    let accent_color: Color = context.theme.role(Role::Accent);

    let status = card_status(view.output, view.player);
    let status_color = match status {
        CardStatus::OutputLost => context.theme.role(Role::Accent2),
        CardStatus::Playing => accent_color,
        CardStatus::Paused => text_color,
        CardStatus::Stopped => dim_color,
    };
    let label = status_label(status);

    let status_base = format!("{} {}", label.glyph, label.word);
    let elapsed_total = elapsed_of(view.position(), view.duration());
    let status_text = format!("{status_base}  {elapsed_total}");
    let status_row = clamp(Rect {
        x: inner.x,
        y: row_y,
        width: status_width,
        height: 1,
    });
    let mut status_spans = vec![
        text(truncate(&status_text, usize::from(status_width)))
            .fg(status_color)
            .into(),
    ];
    let indicator_spans = speed_chip_spans(
        view.speed,
        context.speed_chip,
        ChipStyle {
            foreground: accent_color,
            ..ChipStyle::from_theme(&context.theme)
        },
    );
    let indicator_width = usize::from(speed_chip_width(view.speed, context.speed_chip));
    if let Some(spans) = indicator_spans.filter(|_| {
        status_text.chars().count() + indicator_width <= usize::from(status_width)
    }) {
        status_spans.extend(spans);
    }
    Paragraph::new(Line::from(status_spans)).render(status_row, buffer);
}

fn paint_meter_row(buffer: &mut Buffer, context: &CompactParts<'_>) {
    let inner = context.inner;
    let clamp = |rect: Rect| rect.intersection(inner);
    let row_width = inner.width;
    let StatusRowGeometry {
        row_y,
        volume_width,
        ..
    } = context.status_row;

    let bar_area = clamp(Rect {
        x: inner.x + row_width.saturating_sub(volume_width),
        y: row_y,
        width: volume_width,
        height: 1,
    });
    Paragraph::new(fill(
        &BarFill::volume(context.view.volume.ratio(), usize::from(bar_area.width)),
        BarStyle::volume(&context.theme),
    ))
    .render(bar_area, buffer);
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::{
        Bounded,
        Moment,
        domain::{
            Output,
            Percent,
            Player,
            Playhead,
            Preload,
            Speed,
            appearance::SpeedChip,
        },
        playlist::PlayOrder,
    };

    use crate::{
        card::{CardView, CompactCard, compact_height},
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered, track},
        theme::{ActiveTheme, ColorDepth},
    };

    #[test]
    fn the_compact_card_shows_title_progress_and_status() {
        let theme = noir();
        let track = track("Moon River");
        let player = Player::Playing {
            track: Arc::clone(&track),
            head: Playhead::anchored(
                Duration::from_secs(30),
                Moment::default(),
                Speed::default(),
            ),
            preload: Preload::None,
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
            queue_length: 1,
            displayed_track: Some(&track),
            output: &output,
            now: Moment::default(),
        };
        let widget = CompactCard {
            view,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            speed_chip: SpeedChip::Always,
        };
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
            queue_length: 0,
            displayed_track: None,
            output: &output,
            now: Moment::default(),
        };
        let widget = CompactCard {
            view,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            speed_chip: SpeedChip::Always,
        };
        let text = rendered(40, compact_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert!(text.contains("No track"), "got {text:?}");
        assert!(text.contains("Stopped"), "got {text:?}");
    }
}
