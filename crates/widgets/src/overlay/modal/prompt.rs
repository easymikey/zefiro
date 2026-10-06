use std::{
    error::Error,
    fmt::{self, Write},
};

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
    overlay::modal::frame::{Modal, ModalAreas, ModalSize},
    primitive::{
        canvas::Canvas,
        span::{line, text},
        truncate::truncate,
    },
    theme::active_theme::ActiveTheme,
};

const MARKER: &str = "> ";
const CURSOR: &str = "_";

struct CharCount(usize);

impl Write for CharCount {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 += text.width();
        Ok(())
    }
}

fn error_width(error: &dyn Error) -> usize {
    let mut count = CharCount(0);
    write!(count, "{error}").map_or(0, |()| count.0)
}

#[derive(Debug)]
pub(crate) enum PromptBody<'a> {
    Entry(&'a str),
    Sentence([&'a str; 5]),
}

impl PromptBody<'_> {
    fn width(&self) -> usize {
        match self {
            PromptBody::Entry(input) => MARKER.width() + input.width() + CURSOR.width(),
            PromptBody::Sentence(parts) => parts.iter().map(|part| part.width()).sum(),
        }
    }

    fn line(&self, width: usize, color: Color) -> Line<'static> {
        match self {
            PromptBody::Entry(input) => line([
                text(MARKER).fg(color),
                text((*input).to_string()).fg(color),
                text(CURSOR).fg(color),
            ]),
            PromptBody::Sentence(parts) => {
                line([text(truncate(&parts.concat(), width).into_owned()).fg(color)])
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct PromptWidget<'a> {
    title: &'static str,
    hint: &'static str,
    min_width: Cells,
    body: PromptBody<'a>,
    error: Option<&'a dyn Error>,
    avoid: &'a [Rect],
    theme: ActiveTheme<'a>,
}

impl PromptWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> ModalAreas {
        self.modal().areas(screen, self.avoid)
    }

    fn modal(&self) -> Modal<'_> {
        let error_width = self.error.map_or(0, error_width);
        let widest = self.title.width().max(self.body.width()).max(error_width);
        let colors = self.theme.colors();
        Modal {
            title: self.title,
            size: ModalSize::Dialog {
                min_width: self.min_width,
                content_width: self
                    .min_width
                    .max(u16::try_from(widest).map_or(self.min_width, Cells)),
                content_rows: Cells(1 + u16::from(self.error.is_some())),
            },
            hint: Some(line([text(self.hint).fg(colors.muted_foreground)])),
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = vec![self.body.line(width, self.theme.colors().foreground)];
        if let Some(error) = self.error {
            lines.push(line([text(
                truncate(&error.to_string(), width).into_owned(),
            )
            .fg(self.theme.alert())]));
        }
        lines
    }
}

impl<'a> PromptWidget<'a> {
    #[must_use]
    pub(crate) fn new(body: PromptBody<'a>, active_theme: ActiveTheme<'a>) -> Self {
        Self {
            title: "",
            hint: "",
            min_width: Cells(0),
            body,
            error: None,
            avoid: &[],
            theme: active_theme,
        }
    }

    #[must_use]
    pub(crate) fn title(mut self, title: &'static str) -> Self {
        self.title = title;
        self
    }

    #[must_use]
    pub(crate) fn hint(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }

    #[must_use]
    pub(crate) fn min_width(mut self, min_width: Cells) -> Self {
        self.min_width = min_width;
        self
    }

    #[must_use]
    pub(crate) fn error<E: Error>(mut self, error: Option<&'a E>) -> Self {
        self.error = error.map(|error| -> &'a dyn Error { error });
        self
    }

    #[must_use]
    pub(crate) fn avoid(mut self, avoid: &'a [Rect]) -> Self {
        self.avoid = avoid;
        self
    }

    pub(crate) fn paint(&self, areas: ModalAreas, canvas: Canvas<'_>) {
        let buffer = canvas.buffer;
        self.modal().paint(areas, buffer);
        if areas.body.width != 0 && areas.body.height != 0 {
            Paragraph::new(self.lines(usize::from(areas.body.width)))
                .render(areas.body, buffer);
        }
    }
}

impl Widget for &PromptWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::time::TimecodeError;

    use crate::{
        overlay::modal::prompt::{PromptBody, PromptWidget, error_width},
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[test]
    fn the_measured_width_of_the_body_and_the_error_is_the_painted_width() {
        let theme = noir();
        let error = TimecodeError::Malformed;
        let widget = PromptWidget::new(
            PromptBody::Sentence(["“", "月の光", "”", " — ", "Debussy"]),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .error(Some(&error));
        let lines = widget.lines(usize::MAX);
        assert_eq!(widget.body.width(), lines[0].width());
        assert_eq!(error_width(&error), lines[1].width());
    }
}
