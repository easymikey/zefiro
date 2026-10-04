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
    overlay::modal::frame::{Modal, ModalAreas, ModalBounds, ModalSize, PlacedModal},
    primitive::{
        canvas::Canvas,
        span::{line, text},
        text::truncate,
    },
    theme::{active_theme::ActiveTheme, colors::Role},
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct PromptStyle {
    pub(crate) foreground: Color,
    pub(crate) muted_foreground: Color,
    pub(crate) border: Color,
    pub(crate) background: Color,
    pub(crate) alert: Color,
}

impl PromptStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            foreground: theme.role(Role::Text),
            muted_foreground: theme.role(Role::Dim),
            border: theme.role(Role::Frame),
            background: theme.role(Role::WindowBackground),
            alert: theme.alert(),
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
    pub(crate) style: PromptStyle,
}

impl PromptWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalAreas {
        self.modal().areas(screen, self.avoid)
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
                    .max(u16::try_from(widest).map_or(self.min_width, Cells)),
                content_lines: Cells(1 + u16::from(self.error.is_some())),
            },
            hint: Some(line([text(self.hint).fg(self.style.muted_foreground)])),
            border: self.style.border,
            window_background: self.style.background,
        }
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = vec![self.body.line(width, self.style.foreground)];
        if let Some(error) = &self.error {
            lines.push(line([
                text(truncate(error, width).into_owned()).fg(self.style.alert)
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

    fn paint(&self, areas: ModalAreas, canvas: Canvas<'_>) {
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

impl Widget for &PromptWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}
