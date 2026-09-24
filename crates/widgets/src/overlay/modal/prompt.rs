use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::frame::{Modal, ModalAreas, ModalBounds, ModalSize, PlacedModal},
    primitive::{
        canvas::Canvas,
        glyphs::TruncateGlyphs,
        span::{row, text},
        text::truncate_to_width,
    },
    theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptGlyphs {
    marker: &'static str,
    cursor: &'static str,
}

impl Default for PromptGlyphs {
    fn default() -> Self {
        Self {
            marker: "> ",
            cursor: "_",
        }
    }
}

#[derive(Debug)]
pub(crate) enum PromptBody<'a> {
    Entry(&'a str),
    Sentence(String),
}

impl PromptBody<'_> {
    fn width(&self) -> usize {
        let glyphs = PromptGlyphs::default();
        match self {
            PromptBody::Entry(input) => {
                glyphs.marker.width() + input.width() + glyphs.cursor.width()
            }
            PromptBody::Sentence(sentence) => sentence.width(),
        }
    }

    fn line(&self, width: usize, color: Color) -> Line<'static> {
        let glyphs = PromptGlyphs::default();
        match self {
            PromptBody::Entry(input) => row([
                text(glyphs.marker).fg(color),
                text((*input).to_string()).fg(color),
                text(glyphs.cursor).fg(color),
            ]),
            PromptBody::Sentence(sentence) => {
                row([text(truncated(sentence, width)).fg(color)])
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct Prompt<'a> {
    pub(crate) title: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) min_width: u16,
    pub(crate) body: PromptBody<'a>,
    pub(crate) error: Option<String>,
    pub(crate) avoid: &'a [Rect],
    pub(crate) theme: ActiveTheme<'a>,
}

fn truncated(text: &str, width: usize) -> String {
    truncate_to_width(text, width, TruncateGlyphs::default()).into_owned()
}

impl Prompt<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalAreas {
        self.modal().frame(screen, self.avoid)
    }

    fn modal(&self) -> Modal<'_> {
        let error_width = self.error.as_deref().map_or(0, UnicodeWidthStr::width);
        let widest = self.title.width().max(self.body.width()).max(error_width);
        Modal {
            title: self.title,
            size: ModalSize::Dialog {
                min_width: self.min_width,
                content_width: self
                    .min_width
                    .max(u16::try_from(widest).unwrap_or(self.min_width)),
                content_lines: 1 + u16::from(self.error.is_some()),
            },
            hint: Some(row([text(self.hint).fg(self.theme.dim())])),
            border: self.theme.frame(),
            window_background: self.theme.window_bg(),
        }
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = vec![self.body.line(width, self.theme.text())];
        if let Some(error) = &self.error {
            lines.push(row([text(truncated(error, width)).fg(self.theme.alert())]));
        }
        lines
    }
}

impl Prompt<'_> {
    pub(crate) fn render_in(&self, areas: ModalAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        self.modal().paint(
            PlacedModal {
                areas,
                bounds: ModalBounds {
                    area,
                    avoid: self.avoid,
                },
            },
            buffer,
        );
        if areas.body.width == 0 || areas.body.height == 0 {
            return;
        }
        Paragraph::new(self.lines(usize::from(areas.body.width)))
            .render(areas.body, buffer);
    }
}

impl Widget for &Prompt<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}
