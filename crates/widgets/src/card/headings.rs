use kernel::domain::{player::Player, transport::OutputStatus};
use ratatui::{
    buffer::Buffer,
    layout::Alignment,
    style::Color,
    text::Span,
    widgets::{Paragraph, Widget},
};

use crate::{
    card::{CardWidget, metrics::CardMetrics},
    primitive::{span::text, truncate::truncate},
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Playing,
    Paused,
    Stopped,
    OutputLost,
}

impl CardStatus {
    #[must_use]
    pub(crate) fn new(output_status: OutputStatus, player: &Player) -> Self {
        match output_status {
            OutputStatus::Lost(..) => Self::OutputLost,
            OutputStatus::Ready => match player {
                Player::Playing { .. } | Player::Loading(..) => Self::Playing,
                Player::Paused { .. } => Self::Paused,
                Player::Stopped => Self::Stopped,
            },
        }
    }

    #[must_use]
    pub(crate) fn color(self, theme: &ActiveTheme<'_>) -> Color {
        let colors = theme.colors();
        match self {
            Self::OutputLost => theme.alert(),
            Self::Playing => colors.accent,
            Self::Paused => colors.foreground,
            Self::Stopped => colors.muted_foreground,
        }
    }

    #[must_use]
    pub(crate) fn label(self) -> StatusLabel {
        match self {
            Self::Playing => StatusLabel { glyph: "\u{25b6}" },
            Self::Paused => StatusLabel { glyph: "\u{23f8}" },
            Self::Stopped => StatusLabel { glyph: "\u{25a0}" },
            Self::OutputLost => StatusLabel { glyph: "\u{26a0}" },
        }
    }

    #[must_use]
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Playing => "\u{25b6} Playing",
            Self::Paused => "\u{23f8} Paused",
            Self::Stopped => "\u{25a0} Stopped",
            Self::OutputLost => "\u{26a0} No output",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatusLabel {
    pub(crate) glyph: &'static str,
}

pub(crate) fn paint(
    buffer: &mut Buffer,
    card_widget: &CardWidget<'_>,
    metrics: &CardMetrics,
) {
    let colors = card_widget.active_theme.colors();

    let title = card_widget.view.title();
    let artist = card_widget.view.artist();

    let status = card_widget.view.status();
    let status_color = status.color(&card_widget.active_theme);
    let status_line = truncate(status.text(), usize::from(metrics.status_row.width));
    let status_span: Span<'_> = text(status_line).fg(status_color).into();
    Paragraph::new(status_span)
        .alignment(Alignment::Right)
        .render(metrics.status_row, buffer);

    let title_span: Span<'_> =
        text(truncate(title, usize::from(metrics.title_row.width)))
            .fg(colors.foreground)
            .bold()
            .into();
    Paragraph::new(title_span).render(metrics.title_row, buffer);

    let artist_span: Span<'_> = text(truncate(artist, metrics.row_width.count()))
        .fg(colors.muted_foreground)
        .into();
    Paragraph::new(artist_span).render(metrics.artist_row, buffer);
}
