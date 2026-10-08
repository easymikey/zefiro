use std::borrow::Cow;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StyledText<'a> {
    content: Cow<'a, str>,
    style: Style,
}

impl<'a> StyledText<'a> {
    #[must_use]
    pub(crate) fn fg(mut self, color: impl Into<Color>) -> Self {
        self.style = self.style.fg(color.into());
        self
    }

    #[must_use]
    pub(crate) fn bg(mut self, color: impl Into<Color>) -> Self {
        self.style = self.style.bg(color.into());
        self
    }

    #[must_use]
    pub(crate) fn bold(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::BOLD);
        self
    }

    #[must_use]
    pub(crate) fn dim(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::DIM);
        self
    }

    #[must_use]
    pub(crate) fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl<'a> From<StyledText<'a>> for Span<'a> {
    fn from(piece: StyledText<'a>) -> Self {
        Span::styled(piece.content, piece.style)
    }
}

#[must_use]
pub(crate) fn text<'a>(content: impl Into<Cow<'a, str>>) -> StyledText<'a> {
    StyledText {
        content: content.into(),
        style: Style::default(),
    }
}

#[must_use]
pub(crate) fn line<'a>(pieces: impl IntoIterator<Item = StyledText<'a>>) -> Line<'a> {
    Line::from(pieces.into_iter().map(Span::from).collect::<Vec<_>>())
}

#[must_use]
pub(crate) fn width<'a>(spans: &'a [Span<'a>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

#[cfg(test)]
mod tests {
    use ratatui::{
        style::{Color, Modifier, Style},
        text::Span,
    };
    use rstest::rstest;

    use crate::primitive::span::{StyledText, text};

    #[rstest]
    #[case::fg_and_bold(
        text("x").fg(Color::Red).bold(),
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    )]
    #[case::bg(text("x").bg(Color::Green), Style::default().bg(Color::Green))]
    #[case::dim(text("x").dim(), Style::default().add_modifier(Modifier::DIM))]
    #[case::style_replaces_the_whole_style(
        text("x").fg(Color::Red).bold().style(Style::default().fg(Color::Magenta)),
        Style::default().fg(Color::Magenta)
    )]
    fn each_style_step_produces_the_expected_span(
        #[case] styled_text: StyledText<'static>,
        #[case] style: Style,
    ) {
        assert_eq!(Span::from(styled_text), Span::styled("x", style));
    }
}
