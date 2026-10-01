use ratatui::{
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
    theme::{ActiveTheme, Role},
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
pub(crate) struct Prompt<'a> {
    pub(crate) title: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) min_width: u16,
    pub(crate) body: PromptBody<'a>,
    pub(crate) error: Option<String>,
    pub(crate) avoid: &'a [Rect],
    pub(crate) theme: ActiveTheme<'a>,
}

impl Prompt<'_> {
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
                    .max(u16::try_from(widest).unwrap_or(self.min_width)),
                content_lines: 1 + u16::from(self.error.is_some()),
            },
            hint: Some(line([text(self.hint).fg(self.theme.role(Role::Dim))])),
            border: self.theme.role(Role::Frame),
            window_background: self.theme.role(Role::WindowBackground),
        }
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = vec![self.body.line(width, self.theme.role(Role::Text))];
        if let Some(error) = &self.error {
            lines.push(line([
                text(truncate(error, width).into_owned()).fg(self.theme.alert())
            ]));
        }
        lines
    }
}

impl<'a> Prompt<'a> {
    #[must_use]
    pub(crate) fn avoiding(self, avoid: &'a [Rect]) -> Self {
        Self { avoid, ..self }
    }

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
