use std::borrow::Cow;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyledText<'a> {
    content: Cow<'a, str>,
    style: Style,
}

impl<'a> StyledText<'a> {
    #[must_use]
    pub fn fg(mut self, color: impl Into<Color>) -> Self {
        self.style = self.style.fg(color.into());
        self
    }

    #[must_use]
    pub fn bg(mut self, color: impl Into<Color>) -> Self {
        self.style = self.style.bg(color.into());
        self
    }

    #[must_use]
    pub fn bold(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::BOLD);
        self
    }

    #[must_use]
    pub fn dim(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::DIM);
        self
    }

    #[must_use]
    pub fn italic(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::ITALIC);
        self
    }

    #[must_use]
    pub fn underlined(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::UNDERLINED);
        self
    }

    #[must_use]
    pub fn reversed(mut self) -> Self {
        self.style = self.style.add_modifier(Modifier::REVERSED);
        self
    }

    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
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
    use std::borrow::Cow;

    use ratatui::{
        style::{Color, Modifier, Style},
        text::Span,
    };

    use crate::primitive::span::{line, text};

    #[test]
    fn fg_and_bold_produce_the_expected_span() {
        let span: Span<'static> = text("hello").fg(Color::Red).bold().into();
        let expected = Span::styled(
            "hello",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        );
        assert_eq!(span, expected);
    }

    #[test]
    fn row_of_three_pieces_yields_a_line_with_three_spans() {
        let line = line([text("a"), text("b").bold(), text("c").fg(Color::Blue)]);
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans.first(), Some(&Span::raw("a")));
        assert_eq!(
            line.spans.get(1),
            Some(&Span::styled(
                "b",
                Style::default().add_modifier(Modifier::BOLD)
            ))
        );
        assert_eq!(
            line.spans.get(2),
            Some(&Span::styled("c", Style::default().fg(Color::Blue)))
        );
    }

    #[test]
    fn borrowed_literal_stays_borrowed() {
        let piece = text("literal");
        assert!(matches!(piece.content, Cow::Borrowed(_)));
    }

    #[test]
    fn bg_sets_the_background_color() {
        let span: Span<'static> = text("x").bg(Color::Green).into();
        assert_eq!(span, Span::styled("x", Style::default().bg(Color::Green)));
    }

    #[test]
    fn dim_italic_underlined_reversed_compose_modifiers() {
        let span: Span<'static> =
            text("x").dim().italic().underlined().reversed().into();
        let expected = Style::default().add_modifier(
            Modifier::DIM
                | Modifier::ITALIC
                | Modifier::UNDERLINED
                | Modifier::REVERSED,
        );
        assert_eq!(span, Span::styled("x", expected));
    }

    #[test]
    fn style_replaces_the_whole_style() {
        let replacement = Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD);
        let span: Span<'static> = text("x").fg(Color::Red).style(replacement).into();
        assert_eq!(span, Span::styled("x", replacement));
    }
}
