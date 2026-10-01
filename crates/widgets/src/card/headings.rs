use std::borrow::Cow;

use kernel::domain::{Output, Player};
use ratatui::{
    buffer::Buffer,
    layout::Alignment,
    style::Color,
    text::Span,
    widgets::{Paragraph, Widget},
};

use crate::{
    card::{Card, CardMetrics},
    primitive::{span::text, text::truncate},
    theme::Role,
};

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
        Output::Lost { .. } => CardStatus::OutputLost,
        Output::Ready => match player {
            Player::Playing { .. } | Player::Loading { .. } => CardStatus::Playing,
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

pub(crate) fn paint(buffer: &mut Buffer, card: &Card<'_>, metrics: &CardMetrics) {
    let text_color: Color = card.theme.role(Role::Text);
    let dim_color: Color = card.theme.role(Role::Dim);
    let accent_color: Color = card.theme.role(Role::Accent);

    let current = card.view.displayed_track;
    let title: Cow<'_, str> =
        current.map_or_else(|| "No track".into(), |track| track.song_title().into());
    let artist = current
        .and_then(|track| track.tags().artist.as_deref())
        .unwrap_or("");

    let status = card_status(card.view.output, card.view.player);
    let status_color = match status {
        CardStatus::OutputLost => card.theme.role(Role::Accent2),
        CardStatus::Playing => accent_color,
        CardStatus::Paused => text_color,
        CardStatus::Stopped => dim_color,
    };
    let label = status_label(status);
    let status_text = format!("{} {}", label.glyph, label.word);
    let status_line = truncate(&status_text, usize::from(metrics.status_row.width));
    let status_span: Span<'_> = text(status_line).fg(status_color).into();
    Paragraph::new(status_span)
        .alignment(Alignment::Right)
        .render(metrics.status_row, buffer);

    let title_span: Span<'_> =
        text(truncate(&title, usize::from(metrics.title_row.width)))
            .fg(text_color)
            .bold()
            .into();
    Paragraph::new(title_span).render(metrics.title_row, buffer);

    let artist_span: Span<'_> = text(truncate(artist, usize::from(metrics.row_width)))
        .fg(dim_color)
        .into();
    Paragraph::new(artist_span).render(metrics.artist_row, buffer);
}
