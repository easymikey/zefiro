use std::{borrow::Cow, error::Error};

use kernel::domain::{geometry::Cells, overlay::Verdict};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    overlay::modal::frame::{Modal, ModalAreas, ModalSize},
    primitive::{
        canvas::Canvas,
        display_width,
        span::{StyledText, line, text},
        truncate::{truncate_head, truncate_owned},
    },
    repaint::Presence,
    theme::active_theme::ActiveTheme,
};

pub(crate) const MARKER: &str = "> ";
pub(crate) const CURSOR: &str = " ";

#[must_use]
pub(crate) fn cursor<'a>(
    input: Cow<'a, str>,
    placeholder: &'a str,
) -> [StyledText<'a>; 2] {
    match placeholder.chars().next() {
        Some(first) if input.is_empty() => {
            let (under, rest) = placeholder.split_at(first.len_utf8());
            [text(under).dim().reversed(), text(rest).dim()]
        }
        Some(_) | None => [text(input), text(CURSOR).reversed()],
    }
}

#[derive(Debug)]
pub(crate) enum PromptBody<'a> {
    Entry(Cow<'a, str>),
    Sentence([&'a str; 5]),
    Form(Vec<Line<'a>>),
}

impl<'a> PromptBody<'a> {
    fn width(&self) -> usize {
        match self {
            PromptBody::Entry(input) => MARKER.width() + input.width() + CURSOR.width(),
            PromptBody::Sentence(parts) => parts.iter().map(|part| part.width()).sum(),
            PromptBody::Form(lines) => lines.iter().map(Line::width).max().unwrap_or(0),
        }
    }

    fn rows(&self) -> usize {
        match self {
            PromptBody::Entry(_) | PromptBody::Sentence(_) => 1,
            PromptBody::Form(lines) => lines.len(),
        }
    }

    fn lines(&self, width: usize, color: Color) -> Vec<Cow<'_, Line<'_>>> {
        match self {
            PromptBody::Entry(input) => {
                let budget = width.saturating_sub(MARKER.width() + CURSOR.width());
                vec![Cow::Owned(
                    line(
                        [text(MARKER)]
                            .into_iter()
                            .chain(cursor(truncate_head(input, budget), "")),
                    )
                    .style(Style::new().fg(color)),
                )]
            }
            PromptBody::Sentence(parts) => vec![Cow::Owned(line([text(
                truncate_owned(parts.concat(), width),
            )
            .fg(color)]))],
            PromptBody::Form(lines) => lines.iter().map(Cow::Borrowed).collect(),
        }
    }
}

#[derive(Debug)]
pub struct PromptWidget<'a> {
    title: &'static str,
    hint: &'static str,
    min_width: Cells,
    body: PromptBody<'a>,
    verdict: Option<Option<Verdict>>,
    error: Option<&'a dyn Error>,
    spinner: Presence,
    rows: Vec<Line<'a>>,
    listing_verdict: Option<Verdict>,
    theme: ActiveTheme<'a>,
}

impl PromptWidget<'_> {
    fn lead_width(&self) -> usize {
        match self.spinner {
            Presence::Shown => 2,
            Presence::Hidden => 0,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, screen: Rect, avoid: &[Rect]) -> ModalAreas {
        self.modal().areas(screen, avoid)
    }

    fn modal(&self) -> Modal<'_> {
        let error_width = self
            .error
            .map_or(0, |error| self.lead_width() + display_width(&error));
        let widest = self
            .title
            .width()
            .max(self.body.width())
            .max(
                self.verdict
                    .flatten()
                    .map_or(0, |verdict| display_width(&verdict)),
            )
            .max(
                self.listing_verdict
                    .map_or(0, |verdict| display_width(&verdict)),
            )
            .max(error_width);
        let rows = self.body.rows()
            + usize::from(self.verdict.is_some() || self.error.is_some())
            + self.rows.len();
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
        let body = self.body.lines(width, colors.foreground);
        let listed = self.listing_verdict.map(|verdict| {
            line([
                text(truncate_owned(verdict.to_string(), width)).fg(self.theme.alert())
            ])
        });
        let verdict =
            self.error
                .map(|error| {
                    let mark = match self.spinner {
                        Presence::Shown => Some(self.theme.spinner.mark(&colors)),
                        Presence::Hidden => None,
                    };
                    let budget = width.saturating_sub(self.lead_width());
                    line(
                        mark.into_iter().flatten().chain([text(truncate_owned(
                            error.to_string(),
                            budget,
                        ))
                        .fg(self.theme.alert())]),
                    )
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
        body.into_iter().chain(verdict.map(Cow::Owned)).chain(
            listed
                .map(Cow::Owned)
                .into_iter()
                .chain(self.rows.iter().map(Cow::Borrowed))
                .take(self.rows.len()),
        )
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
            verdict: None,
            error: None,
            spinner: Presence::Hidden,
            rows: Vec::new(),
            listing_verdict: None,
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
    pub(crate) fn rows(mut self, rows: Vec<Line<'a>>) -> Self {
        self.rows = rows;
        self
    }

    #[must_use]
    pub(crate) fn listing_verdict(mut self, listing_verdict: Option<Verdict>) -> Self {
        self.listing_verdict = listing_verdict;
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

    #[must_use]
    pub(crate) fn spinner(mut self, presence: Presence) -> Self {
        self.spinner = presence;
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
    use ratatui::style::Modifier;
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

    #[test]
    fn an_entry_ends_in_a_block_cursor_of_one_reversed_cell() {
        let theme = noir();
        let widget = PromptWidget::new(
            PromptBody::Entry(Cow::Borrowed("月の光")),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let lines: Vec<_> = widget.lines(usize::MAX).collect();
        let cursor = lines[0].spans.last().unwrap();
        assert_eq!(
            (
                cursor.content.as_ref(),
                cursor.style.add_modifier.contains(Modifier::REVERSED)
            ),
            (" ", true)
        );
    }
}
