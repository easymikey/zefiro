use std::{borrow::Cow, error::Error};

use kernel::domain::{geometry::Cells, overlay::Verdict};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::frame::{Modal, ModalAreas, ModalSize},
    primitive::{
        canvas::Canvas,
        display_width,
        span::{line, text},
        truncate::{truncate, truncate_head, truncate_owned},
    },
    theme::active_theme::ActiveTheme,
};

const MARKER: &str = "> ";
const CURSOR: &str = "_";

#[derive(Debug)]
pub(crate) enum PromptBody<'a> {
    Entry(Cow<'a, str>),
    Sentence([&'a str; 5]),
}

impl<'a> PromptBody<'a> {
    fn width(&self) -> usize {
        match self {
            PromptBody::Entry(input) => MARKER.width() + input.width() + CURSOR.width(),
            PromptBody::Sentence(parts) => parts.iter().map(|part| part.width()).sum(),
        }
    }

    fn line(&self, width: usize, color: Color) -> Line<'_> {
        match self {
            PromptBody::Entry(input) => {
                let budget = width.saturating_sub(MARKER.width() + CURSOR.width());
                line([
                    text(MARKER).fg(color),
                    text(truncate_head(input, budget)).fg(color),
                    text(CURSOR).fg(color),
                ])
            }
            PromptBody::Sentence(parts) => {
                line([text(truncate_owned(parts.concat(), width)).fg(color)])
            }
        }
    }
}

#[derive(Debug)]
pub struct PromptWidget<'a> {
    title: &'static str,
    hint: &'static str,
    min_width: Cells,
    answers: Vec<Line<'a>>,
    body: PromptBody<'a>,
    field_hint: Option<&'static str>,
    verdict: Option<Option<Verdict>>,
    error: Option<&'a dyn Error>,
    theme: ActiveTheme<'a>,
}

impl PromptWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect, avoid: &[Rect]) -> ModalAreas {
        self.modal().areas(screen, avoid)
    }

    fn modal(&self) -> Modal<'_> {
        let error_width = self.error.map_or(0, |error| display_width(&error));
        let field_hint_width = self.field_hint.map_or(0, UnicodeWidthStr::width);
        let widest = self
            .answers
            .iter()
            .map(Line::width)
            .fold(self.title.width().max(self.body.width()), usize::max)
            .max(field_hint_width)
            .max(
                self.verdict
                    .flatten()
                    .map_or(0, |verdict| display_width(&verdict)),
            )
            .max(error_width);
        let rows = self.answers.len()
            + 1
            + usize::from(self.field_hint.is_some())
            + usize::from(self.verdict.is_some() || self.error.is_some());
        let colors = self.theme.colors();
        Modal {
            title: self.title,
            size: ModalSize::Dialog {
                min_width: self.min_width,
                content_width: self
                    .min_width
                    .max(u16::try_from(widest).map_or(self.min_width, Cells)),
                content_rows: u16::try_from(rows).map_or(Cells(u16::MAX), Cells),
            },
            hint: Some(line([text(self.hint).fg(colors.muted_foreground)])),
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    fn lines(&self, width: usize) -> impl Iterator<Item = Cow<'_, Line<'_>>> {
        let colors = self.theme.colors();
        let body = self.body.line(width, colors.foreground);
        let field_hint = self.field_hint.map(|field_hint| {
            line([text(truncate(field_hint, width)).fg(colors.muted_foreground)])
        });
        let verdict =
            self.error
                .map(|error| {
                    line([text(truncate_owned(error.to_string(), width))
                        .fg(self.theme.alert())])
                })
                .or_else(|| {
                    self.verdict.map(|shown| {
                        shown.map_or_else(Line::default, |verdict| {
                            let color = match verdict {
                                Verdict::Readable => colors.muted_foreground,
                                Verdict::Missing
                                | Verdict::NotADirectory
                                | Verdict::Denied
                                | Verdict::Unreadable(_) => self.theme.alert(),
                            };
                            line([text(truncate_owned(verdict.to_string(), width))
                                .fg(color)])
                        })
                    })
                });
        self.answers
            .iter()
            .map(Cow::Borrowed)
            .chain([Cow::Owned(body)])
            .chain(field_hint.map(Cow::Owned))
            .chain(verdict.map(Cow::Owned))
    }
}

impl<'a> PromptWidget<'a> {
    #[must_use]
    pub(crate) fn new(body: PromptBody<'a>, active_theme: ActiveTheme<'a>) -> Self {
        Self {
            title: "",
            hint: "",
            min_width: Cells(0),
            answers: Vec::new(),
            body,
            field_hint: None,
            verdict: None,
            error: None,
            theme: active_theme,
        }
    }

    #[must_use]
    pub(crate) fn field_hint(mut self, field_hint: &'static str) -> Self {
        self.field_hint = Some(field_hint);
        self
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
    pub(crate) fn answers(mut self, answers: Vec<Line<'a>>) -> Self {
        self.answers = answers;
        self
    }

    #[must_use]
    pub(crate) fn verdict(mut self, verdict: Option<Verdict>) -> Self {
        self.verdict = Some(verdict);
        self
    }

    #[must_use]
    pub(crate) fn error<E: Error>(mut self, error: Option<&'a E>) -> Self {
        self.error = error.map(|error| -> &'a dyn Error { error });
        self
    }

    pub(crate) fn paint(&self, areas: ModalAreas, canvas: Canvas<'_>) {
        let buffer = canvas.buffer;
        self.modal().paint(areas, buffer);
        let body = areas.body;
        if body.width != 0 && body.height != 0 {
            for (line, row) in self.lines(usize::from(body.width)).zip(body.rows()) {
                buffer.set_line(row.x, row.y, &line, row.width);
            }
        }
    }
}

impl Widget for &PromptWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area, &[]), Canvas { area, buffer });
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use kernel::domain::time::TimecodeError;
    use rstest::rstest;

    use crate::{
        overlay::modal::prompt::{PromptBody, PromptWidget},
        primitive::display_width,
        test_support::noir,
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[rstest]
    #[case::sentence(PromptBody::Sentence(["“", "月の光", "”", " — ", "Debussy"]))]
    #[case::entry(PromptBody::Entry(Cow::Borrowed("月の光")))]
    fn the_measured_width_of_the_body_and_the_error_is_the_painted_width(
        #[case] body: PromptBody<'static>,
    ) {
        let theme = noir();
        let error = TimecodeError::Malformed;
        let widget =
            PromptWidget::new(body, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .error(Some(&error));
        let lines: Vec<_> = widget.lines(usize::MAX).collect();
        assert_eq!(widget.body.width(), lines[0].width());
        assert_eq!(display_width(&error), lines[1].width());
    }
}
