use kernel::domain::geometry::Cells;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::{
        frame::{Modal, ModalSize},
        placement::OverlayAreas,
    },
    primitive::{
        canvas::Canvas,
        span::{line, text},
        text::truncate,
    },
    theme::active_theme::ActiveTheme,
};

const MARKER: &str = "> ";
const CURSOR: &str = "_";

#[derive(Debug)]
pub(crate) enum PromptBody<'a> {
    Entry(&'a str),
    Sentence(String),
}

impl PromptBody<'_> {
    fn width(&self) -> usize {
        match self {
            PromptBody::Entry(input) => MARKER.width() + input.width() + CURSOR.width(),
            PromptBody::Sentence(sentence) => sentence.width(),
        }
    }

    fn line(&self, width: usize, color: Color) -> Line<'static> {
        match self {
            PromptBody::Entry(input) => line([
                text(MARKER).fg(color),
                text((*input).to_string()).fg(color),
                text(CURSOR).fg(color),
            ]),
            PromptBody::Sentence(sentence) => {
                line([text(truncate(sentence, width).into_owned()).fg(color)])
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct PromptWidget<'a> {
    pub(crate) title: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) min_width: Cells,
    pub(crate) body: PromptBody<'a>,
    pub(crate) error: Option<String>,
    pub(crate) avoid: &'a [Rect],
    pub(crate) theme: ActiveTheme<'a>,
}

impl PromptWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        OverlayAreas::Dialog(self.modal().areas(screen, self.avoid))
    }

    fn modal(&self) -> Modal<'_> {
        let error_width = self.error.as_deref().map_or(0, UnicodeWidthStr::width);
        let widest = self.title.width().max(self.body.width()).max(error_width);
        let colors = self.theme.colors();
        Modal {
            title: self.title,
            size: ModalSize::Dialog {
                min_width: self.min_width,
                content_width: self
                    .min_width
                    .max(u16::try_from(widest).map_or(self.min_width, Cells)),
                content_lines: Cells(1 + u16::from(self.error.is_some())),
            },
            hint: Some(line([text(self.hint).fg(colors.muted_foreground)])),
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = vec![self.body.line(width, self.theme.colors().text)];
        if let Some(error) = &self.error {
            lines.push(line([
                text(truncate(error, width).into_owned()).fg(self.theme.alert())
            ]));
        }
        lines
    }
}

impl<'a> PromptWidget<'a> {
    #[must_use]
    pub(crate) fn avoiding(self, avoid: &'a [Rect]) -> Self {
        Self { avoid, ..self }
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let OverlayAreas::Dialog(areas) = areas else {
            return;
        };
        let buffer = canvas.buffer;
        self.modal().paint(areas, buffer);
        if areas.body.width == 0 || areas.body.height == 0 {
            return;
        }
        Paragraph::new(self.lines(usize::from(areas.body.width)))
            .render(areas.body, buffer);
    }
}

impl Widget for &PromptWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}
