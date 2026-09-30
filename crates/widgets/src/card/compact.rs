use config::SpeedChip;
use raster::unit_fraction;
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
    primitive::{
        bar::{BarFill, fill_line},
        chip::{ChipColors, speed_chip_spans, speed_chip_width},
        relative_time::elapsed_of,
        span::{line, text},
        text::truncate,
    },
    theme::{ActiveTheme, FillColors},
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct CompactCardLayout {
    padding: u16,
    border_width: u16,
    title_rows: u16,
    progress_rows: u16,
    status_rows: u16,
    volume_bar_width: u16,
}

impl Default for CompactCardLayout {
    fn default() -> Self {
        Self {
            padding: 1,
            border_width: 2,
            title_rows: 2,
            progress_rows: 1,
            status_rows: 1,
            volume_bar_width: 16,
        }
    }
}

#[must_use]
pub(crate) fn compact_height() -> u16 {
    let layout = CompactCardLayout::default();
    layout.border_width + layout.title_rows + layout.progress_rows + layout.status_rows
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
    layout: CompactCardLayout,
    inner: Rect,
    status_row: StatusRowGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusRowGeometry {
    row_y: u16,
    status_width: u16,
    volume_width: u16,
}

fn status_row_geometry(inner: Rect, layout: CompactCardLayout) -> StatusRowGeometry {
    let row_width = inner.width;
    let volume_width = layout.volume_bar_width.min(row_width / 2);
    StatusRowGeometry {
        row_y: inner.y + layout.title_rows + layout.progress_rows,
        status_width: row_width.saturating_sub(volume_width + 1),
        volume_width,
    }
}

fn content_area(area: Rect, layout: CompactCardLayout) -> Rect {
    let inner = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .inner(area);
    Rect {
        x: inner.x + layout.padding,
        y: inner.y,
        width: inner.width.saturating_sub(layout.padding * 2),
        height: inner.height,
    }
}

#[must_use]
pub(crate) fn progress_bar_width(area: Rect) -> u16 {
    content_area(area, CompactCardLayout::default()).width
}

impl Widget for &CompactCard<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let layout = CompactCardLayout::default();
        let theme = self.theme;
        let frame_color: Color = theme.border();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(frame_color))
            .title(" Sifr ")
            .title_style(Style::default().fg(frame_color));
        let inner = content_area(area, layout);
        block.render(area, buffer);

        let context = CompactParts {
            view: self.view,
            theme,
            speed_chip: self.speed_chip,
            layout,
            inner,
            status_row: status_row_geometry(inner, layout),
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
    let text_color: Color = context.theme.text();
    let dim_color: Color = context.theme.dim();

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
    let accent_color: Color = context.theme.accent();
    let dim_color: Color = context.theme.dim();

    let current = context.view.displayed_track;
    let duration = current
        .and_then(|track| track.duration())
        .unwrap_or_default();
    let position = context.view.player.position_at(context.view.now);
    let fraction = if duration.is_zero() {
        0.0
    } else {
        position.as_secs_f64() / duration.as_secs_f64()
    };

    let progress_y = inner.y + context.layout.title_rows;
    let progress_row = clamp(Rect {
        x: inner.x,
        y: progress_y,
        width: row_width,
        height: 1,
    });
    Paragraph::new(fill_line(
        &BarFill::progress(unit_fraction(fraction), usize::from(row_width)),
        FillColors {
            accent: accent_color,
            dim: dim_color,
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
    let text_color: Color = context.theme.text();
    let dim_color: Color = context.theme.dim();
    let accent_color: Color = context.theme.accent();

    let status = card_status(view.output, view.player);
    let status_color = match status {
        CardStatus::OutputLost => context.theme.secondary_accent(),
        CardStatus::Playing => accent_color,
        CardStatus::Paused => text_color,
        CardStatus::Stopped => dim_color,
    };
    let label = status_label(status);

    let position = view.player.position_at(view.now);
    let duration = view
        .displayed_track
        .and_then(|track| track.duration())
        .unwrap_or_default();
    let status_base = format!("{} {}", label.glyph, label.word);
    let elapsed_total = elapsed_of(position, duration);
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
        ChipColors {
            border: dim_color,
            value: accent_color,
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
    Paragraph::new(fill_line(
        &BarFill::volume(context.view.volume.ratio(), usize::from(bar_area.width)),
        context.theme.volume_fill_colors(),
    ))
    .render(bar_area, buffer);
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use config::SpeedChip;
    use kernel::{
        Bounded,
        Moment,
        domain::{
            AudioFormat,
            Output,
            Percent,
            Player,
            Playhead,
            Preload,
            Speed,
            Tags,
            Track,
        },
        playlist::PlayOrder,
    };

    use crate::{
        card::{CardView, CompactCard, compact_height},
        scene::fixtures::{noir, painted},
        spectrum::{SPECTRUM_BANDS, Spectrum},
        theme::{ActiveTheme, ColorDepth},
    };

    fn track(title: &str, duration_secs: u64) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("/music/{title}.mp3"))
                .duration(Duration::from_secs(duration_secs))
                .tags(Tags {
                    title: Some(title.to_string()),
                    artist: Some("Test Artist".to_string()),
                    ..Tags::default()
                })
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

    #[test]
    fn the_compact_card_shows_title_progress_and_status() {
        let theme = noir();
        let track = track("Moon River", 245);
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
        let text = painted(&widget, 40, compact_height());
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
        let text = painted(&widget, 40, compact_height());
        assert!(text.contains("No track"), "got {text:?}");
        assert!(text.contains("Stopped"), "got {text:?}");
    }
}
