use kernel::domain::{appearance::SpeedChip, geometry::Cells};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::{
    card::{CardView, card_frame},
    primitive::{
        bar::BarFill,
        chip::speed_chip_spans,
        span::{self, line, text},
        time_text::elapsed_text,
        truncate::{truncate, truncate_line},
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
    progress_bar_width: Cells,
}

impl<'a> CompactCardWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        view: CardView<'a>,
        theme: ActiveTheme<'a>,
        progress_bar_width: Cells,
    ) -> Self {
        Self {
            view,
            theme,
            speed_chip: SpeedChip::default(),
            progress_bar_width,
        }
    }

    #[must_use]
    pub(crate) fn speed_chip(mut self, speed_chip: SpeedChip) -> Self {
        self.speed_chip = speed_chip;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompactCardAreas {
    title: Rect,
    artist: Rect,
    progress: Rect,
    status: Rect,
    meter: Rect,
}

impl CompactCardAreas {
    fn new(inner: Rect) -> Self {
        let row_width = inner.width;
        let volume_width = VOLUME_BAR_WIDTH.min(row_width / 2);
        let status_width = row_width.saturating_sub(volume_width + 1);
        let row = inner.y + TITLE_ROWS + PROGRESS_ROWS;
        let clamp = |rect: Rect| rect.intersection(inner);
        Self {
            title: clamp(Rect {
                x: inner.x,
                y: inner.y,
                width: row_width,
                height: 1,
            }),
            artist: clamp(Rect {
                x: inner.x,
                y: inner.y + 1,
                width: row_width,
                height: 1,
            }),
            progress: clamp(Rect {
                x: inner.x,
                y: inner.y + TITLE_ROWS,
                width: row_width,
                height: 1,
            }),
            status: clamp(Rect {
                x: inner.x,
                y: row,
                width: status_width,
                height: 1,
            }),
            meter: clamp(Rect {
                x: inner.x + row_width.saturating_sub(volume_width),
                y: row,
                width: volume_width,
                height: 1,
            }),
        }
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
pub(crate) fn progress_bar_width(area: Rect) -> Cells {
    Cells(content_area(area).width)
}

impl Widget for &CompactCardWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.theme;
        let frame_color: Color = theme.colors().muted_foreground;

        let inner = content_area(area);
        card_frame(frame_color).render(area, buffer);

        let compact_card_areas = CompactCardAreas::new(inner);
        paint_header_row(buffer, self, &compact_card_areas);
        paint_progress_row(buffer, self, &compact_card_areas);
        paint_status_row(buffer, self, &compact_card_areas);
        paint_meter_row(buffer, self, &compact_card_areas);
    }
}

fn paint_header_row(
    buffer: &mut Buffer,
    compact_card_widget: &CompactCardWidget<'_>,
    compact_card_areas: &CompactCardAreas,
) {
    let colors = compact_card_widget.theme.colors();
    let title_row = compact_card_areas.title;
    let artist_row = compact_card_areas.artist;

    let title = compact_card_widget.view.title();
    let artist = compact_card_widget.view.artist();

    Paragraph::new(line([text(truncate(title, usize::from(title_row.width)))
        .fg(colors.foreground)
        .bold()]))
    .render(title_row, buffer);

    Paragraph::new(line([text(truncate(
        artist,
        usize::from(artist_row.width),
    ))
    .fg(colors.muted_foreground)]))
    .render(artist_row, buffer);
}

fn paint_progress_row(
    buffer: &mut Buffer,
    compact_card_widget: &CompactCardWidget<'_>,
    compact_card_areas: &CompactCardAreas,
) {
    Paragraph::new(
        BarFill::progress(
            compact_card_widget.view.progress_fraction(),
            compact_card_widget.progress_bar_width,
        )
        .line(
            compact_card_widget.theme.progress_fill(),
            compact_card_widget.theme.progress_groove(),
        ),
    )
    .render(compact_card_areas.progress, buffer);
}

fn paint_status_row(
    buffer: &mut Buffer,
    compact_card_widget: &CompactCardWidget<'_>,
    compact_card_areas: &CompactCardAreas,
) {
    let view = compact_card_widget.view;
    let status = view.status();
    let status_color = status.color(&compact_card_widget.theme);
    let elapsed_total = elapsed_text(view.position(), view.duration());
    let status_line = line([
        status.mark(&compact_card_widget.theme),
        text(" ").fg(status_color),
        text(status.word()).fg(status_color),
        text("  ").fg(status_color),
        text(elapsed_total).fg(status_color),
    ]);
    let indicator_spans = speed_chip_spans(
        view.speed,
        compact_card_widget.speed_chip,
        &compact_card_widget.theme.colors(),
    );
    let indicator_width = span::width(&indicator_spans);
    let status_width = usize::from(compact_card_areas.status.width);
    let fits = status_line.width() + indicator_width <= status_width;
    let status_spans: Vec<_> = truncate_line(status_line, status_width)
        .spans
        .into_iter()
        .chain(indicator_spans.into_iter().filter(|_| fits))
        .collect();
    Paragraph::new(Line::from(status_spans)).render(compact_card_areas.status, buffer);
}

fn paint_meter_row(
    buffer: &mut Buffer,
    compact_card_widget: &CompactCardWidget<'_>,
    compact_card_areas: &CompactCardAreas,
) {
    Paragraph::new(
        BarFill::volume(
            compact_card_widget.view.volume.ratio(),
            Cells(compact_card_areas.meter.width),
        )
        .line(
            compact_card_widget.theme.colors().accent,
            compact_card_widget.theme.colors().bar_groove,
        ),
    )
    .render(compact_card_areas.meter, buffer);
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::SpeedChip,
        bounded::Bounded,
        percent::Percent,
        player::Player,
        playlist::PlayOrder,
        speed::Speed,
        time::Moment,
        transport::OutputStatus,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        card::{
            CardView,
            compact::{CompactCardWidget, compact_height, progress_bar_width},
        },
        repaint::Presence,
        spectrum::{SPECTRUM_BANDS, Spectrum},
        test_support::{noir, rendered, track},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[rstest]
    #[case::room_for_both(64, Presence::Shown)]
    #[case::room_for_the_status_alone(49, Presence::Hidden)]
    fn the_speed_chip_follows_the_status_only_when_both_fit(
        #[case] width: u16,
        #[case] presence: Presence,
    ) {
        let theme = noir();
        let player = Player::Stopped;
        let spectrum: Spectrum = [0.0; SPECTRUM_BANDS];
        let play_order = PlayOrder::default();
        let output_status = OutputStatus::Ready;
        let displayed_track = track("Moon River");
        let view = CardView {
            player: &player,
            speed: Speed::clamped(1.25),
            volume: Percent::clamped(70),
            spectrum: &spectrum,
            repeat_mode: Default::default(),
            play_order: &play_order,
            displayed_track: Some(&displayed_track),
            output_status: &output_status,
            buffering_revision: None,
            now: Moment::default(),
        };
        let area = Rect::new(0, 0, width, compact_height());
        let widget = CompactCardWidget::new(
            view,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
            progress_bar_width(area),
        )
        .speed_chip(SpeedChip::Always);
        let text = rendered(width, compact_height(), |frame| {
            frame.render_widget(&widget, frame.area());
        })
        .to_string();
        assert_eq!(
            Presence::from(text.contains('\u{00BB}')),
            presence,
            "got {text:?}"
        );
    }
}
