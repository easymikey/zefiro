use kernel::domain::{player::Player, transport::Output};
use ratatui::{
    buffer::Buffer,
    layout::Alignment,
    style::Color,
    text::Span,
    widgets::{Paragraph, Widget},
};

use crate::{
    card::{CardWidget, metrics::CardMetrics},
    primitive::{span::text, text::truncate},
    theme::active_theme::ActiveTheme,
};

#[must_use]
pub(crate) fn status_color(theme: &ActiveTheme<'_>, status: CardStatus) -> Color {
    let colors = theme.colors();
    match status {
        CardStatus::OutputLost => theme.alert(),
        CardStatus::Playing => colors.accent,
        CardStatus::Paused => colors.text,
        CardStatus::Stopped => colors.muted_foreground,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Playing,
    Paused,
    Stopped,
    OutputLost,
}

#[must_use]
pub(crate) fn card_status(output: &Output, player: &Player) -> CardStatus {
    match output {
        Output::Lost(..) => CardStatus::OutputLost,
        Output::Ready => match player {
            Player::Playing { .. } | Player::Loading(..) => CardStatus::Playing,
            Player::Paused { .. } => CardStatus::Paused,
            Player::Stopped => CardStatus::Stopped,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatusLabel {
    pub(crate) glyph: &'static str,
    pub(crate) word: &'static str,
}

#[must_use]
pub(crate) fn status_label(status: CardStatus) -> StatusLabel {
    match status {
        CardStatus::Playing => StatusLabel {
            glyph: "\u{25b6}",
            word: "Playing",
        },
        CardStatus::Paused => StatusLabel {
            glyph: "\u{23f8}",
            word: "Paused",
        },
        CardStatus::Stopped => StatusLabel {
            glyph: "\u{25a0}",
            word: "Stopped",
        },
        CardStatus::OutputLost => StatusLabel {
            glyph: "\u{26a0}",
            word: "No output",
        },
    }
}

pub(crate) fn paint(buffer: &mut Buffer, card: &CardWidget<'_>, metrics: &CardMetrics) {
    let colors = card.theme.colors();

    let title = card.view.title();
    let artist = card.view.artist();

    let status = card_status(card.view.output, card.view.player);
    let status_color = status_color(&card.theme, status);
    let label = status_label(status);
    let status_text = format!("{} {}", label.glyph, label.word);
    let status_line = truncate(&status_text, usize::from(metrics.status_row.width));
    let status_span: Span<'_> = text(status_line).fg(status_color).into();
    Paragraph::new(status_span)
        .alignment(Alignment::Right)
        .render(metrics.status_row, buffer);

    let title_span: Span<'_> =
        text(truncate(title, usize::from(metrics.title_row.width)))
            .fg(colors.text)
            .bold()
            .into();
    Paragraph::new(title_span).render(metrics.title_row, buffer);

    let artist_span: Span<'_> = text(truncate(artist, metrics.row_width.count()))
        .fg(colors.muted_foreground)
        .into();
    Paragraph::new(artist_span).render(metrics.artist_row, buffer);
}
