use kernel::domain::{player::Player, revision::Revision, transport::OutputStatus};
use ratatui::{
    buffer::Buffer,
    layout::Alignment,
    style::Color,
    text::Span,
    widgets::{Paragraph, Widget},
};

use crate::{
    card::{CardWidget, metrics::CardMetrics},
    primitive::{
        span::{line, text},
        truncate::{truncate, truncate_line},
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Playing,
    Buffering,
    Paused,
    Stopped,
    OutputLost,
}

const BUFFERING_GLYPH: &str = "\u{25cc}";

impl CardStatus {
    #[must_use]
    pub(crate) fn new(
        output_status: OutputStatus,
        buffering_revision: Option<Revision>,
        player: &Player,
    ) -> Self {
        match output_status {
            OutputStatus::Lost(..) => Self::OutputLost,
            OutputStatus::Ready => match player {
                Player::Playing { .. } | Player::Loading(..) => {
                    if buffering_revision.is_some() {
                        Self::Buffering
                    } else {
                        Self::Playing
                    }
                }
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
            Self::Buffering | Self::Stopped => colors.muted_foreground,
        }
    }

    #[must_use]
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Self::Playing => "\u{25b6}",
            Self::Buffering => BUFFERING_GLYPH,
            Self::Paused => "\u{23f8}",
            Self::Stopped => "\u{25a0}",
            Self::OutputLost => "\u{26a0}",
        }
    }

    #[must_use]
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Buffering => "Buffering",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
            Self::OutputLost => "No output",
        }
    }
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
    let status_line = truncate_line(
        line([
            text(status.glyph()).fg(status_color),
            text(format!(" {}", status.word())).fg(status_color),
        ]),
        usize::from(metrics.status_row.width),
    );
    Paragraph::new(status_line)
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
