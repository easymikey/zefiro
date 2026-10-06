use kernel::domain::toast::{Toast, ToastLevel};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{canvas::Canvas, inset::Inset, text::truncate},
    screen::breakpoint::Breakpoint,
    theme::active_theme::ActiveTheme,
};

const TOAST_WIDTH: u16 = 42;
const GAP: u16 = 1;
const INSET_CELLS: u16 = 1;
const CHROME_CELLS: u16 = 4;
const BORDER_ROWS: u16 = 2;
const TITLE_ROWS: u16 = 1;
const MAX_TEXT_ROWS: usize = 3;

const CARD_INSET: Inset = Inset {
    top: 0,
    left: 1,
    right: 1,
    bottom: 0,
};

#[must_use]
pub(crate) fn accent(theme: &ActiveTheme<'_>, kind: ToastLevel) -> Color {
    match kind {
        ToastLevel::Info => theme.colors().accent,
        ToastLevel::Error => theme.alert(),
    }
}

fn icon(kind: ToastLevel) -> &'static str {
    match kind {
        ToastLevel::Info => "i",
        ToastLevel::Error => "\u{2717}",
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToastWidget<'a> {
    toasts: &'a [Toast],
    theme: ActiveTheme<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Stack,
    Line,
}

impl Form {
    fn of(breakpoint: Breakpoint) -> Self {
        match breakpoint {
            Breakpoint::Full | Breakpoint::Compact => Form::Stack,
            Breakpoint::Minimal | Breakpoint::TooSmall => Form::Line,
        }
    }

    fn painted(area: Rect) -> Self {
        if area.height == 1 {
            Form::Line
        } else {
            Form::Stack
        }
    }
}

struct Placed<'a> {
    toast: &'a Toast,
    rect: Rect,
    lines: Vec<String>,
}

fn wrapped_paragraph(text: &str, width: usize) -> Vec<String> {
    text.split_whitespace()
        .fold(Vec::new(), |mut lines: Vec<String>, word| {
            match lines.last_mut() {
                Some(line) if line.width() + 1 + word.width() <= width => {
                    line.push(' ');
                    line.push_str(word);
                }
                Some(_) | None => lines.push(word.to_owned()),
            }
            lines
        })
}

fn wrapped(text: &str, width: usize) -> Vec<String> {
    text.lines()
        .flat_map(|line| wrapped_paragraph(line, width))
        .collect()
}

fn fitted(text: &str, width: usize, rows: usize) -> Vec<String> {
    let clipped = |line: &String| truncate(line, width).into_owned();
    let lines = wrapped(text, width);
    if lines.len() <= rows {
        return lines.iter().map(clipped).collect();
    }
    let head = rows.saturating_sub(1);
    let tail = lines
        .get(head..)
        .map_or_else(String::new, |rest| rest.join(" "));
    lines
        .iter()
        .take(head)
        .map(clipped)
        .chain(std::iter::once(truncate(&tail, width).into_owned()))
        .collect()
}

fn title_line(toast: &Toast, room: usize) -> String {
    let line = format!("{} {}", icon(toast.kind), toast.title);
    truncate(&line, room).into_owned()
}

impl<'a> ToastWidget<'a> {
    #[must_use]
    pub(crate) fn new(toasts: &'a [Toast], active_theme: ActiveTheme<'a>) -> Self {
        Self {
            toasts,
            theme: active_theme,
        }
    }

    fn placed(&self, screen: Rect, form: Form) -> Vec<Placed<'a>> {
        match form {
            Form::Stack => self.stacked(screen),
            Form::Line => self.line(screen),
        }
    }

    fn line(&self, screen: Rect) -> Vec<Placed<'a>> {
        self.toasts
            .first()
            .map(|toast| {
                let title = title_line(toast, usize::from(screen.width));
                let width = u16::try_from(title.width()).unwrap_or(screen.width);
                Placed {
                    toast,
                    rect: Rect {
                        x: screen.right().saturating_sub(width),
                        y: screen.y,
                        width,
                        height: 1,
                    },
                    lines: vec![title],
                }
            })
            .into_iter()
            .collect()
    }

    fn stacked(&self, screen: Rect) -> Vec<Placed<'a>> {
        let width = TOAST_WIDTH.min(screen.width.saturating_sub(INSET_CELLS * 2));
        let Some(room) = width.checked_sub(CHROME_CELLS).filter(|room| *room > 0)
        else {
            return Vec::new();
        };
        let x = screen.right().saturating_sub(INSET_CELLS + width);
        self.toasts
            .iter()
            .scan(screen.y.saturating_add(INSET_CELLS), |top, toast| {
                let text = fitted(
                    toast.text.as_deref().map_or("", str::trim),
                    usize::from(room),
                    MAX_TEXT_ROWS,
                );
                let rows = u16::try_from(text.len()).unwrap_or(0);
                let height = BORDER_ROWS + TITLE_ROWS + rows;
                (top.saturating_add(height) <= screen.bottom()).then(|| {
                    let rect = Rect::new(x, *top, width, height);
                    *top = top.saturating_add(height + GAP);
                    Placed {
                        toast,
                        rect,
                        lines: std::iter::once(title_line(toast, usize::from(room)))
                            .chain(text)
                            .collect(),
                    }
                })
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn area(self, screen: Rect, breakpoint: Breakpoint) -> Option<Rect> {
        self.placed(screen, Form::of(breakpoint))
            .iter()
            .map(|placed| placed.rect)
            .reduce(Rect::union)
    }

    pub(crate) fn paint(self, toast_area: Rect, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        for placed in self.placed(area, Form::painted(toast_area)) {
            self.paint_toast(&placed, buffer);
        }
    }

    fn paint_toast(&self, placed: &Placed<'_>, buffer: &mut Buffer) {
        let accent = accent(&self.theme, placed.toast.kind);
        let colors = self.theme.colors();
        let rect = placed.rect;
        Clear.render(rect, buffer);
        Block::new()
            .style(
                Style::default()
                    .bg(colors.window_background)
                    .fg(colors.text),
            )
            .render(rect, buffer);
        let (title_text, body_lines) = placed
            .lines
            .split_first()
            .map_or(("", &[][..]), |(title, rest)| (title.as_str(), rest));
        let title = Paragraph::new(title_text)
            .style(Style::default().fg(accent).add_modifier(Modifier::BOLD));
        if rect.height == 1 {
            title.render(rect, buffer);
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(accent))
            .padding(CARD_INSET.padding());
        let inner = block.inner(rect);
        block.render(rect, buffer);
        title.render(Rect { height: 1, ..inner }, buffer);
        let body = body_lines.join("\n");
        let below = Rect {
            y: inner.y.saturating_add(1),
            height: inner.height.saturating_sub(1),
            ..inner
        };
        Paragraph::new(body)
            .style(Style::default().fg(colors.text))
            .render(below, buffer);
    }
}

impl Widget for ToastWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(toast_area) = self.area(area, Breakpoint::Full) {
            self.paint(toast_area, Canvas { area, buffer });
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::toast::{Toast, ToastLevel};
    use ratatui::layout::Rect;

    use crate::{
        screen::breakpoint::Breakpoint,
        test_support::{noir, rendered},
        theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
        toast::{ToastWidget, accent, icon},
    };

    fn toaster<'a>(toasts: &'a [Toast], theme: &'a Theme) -> ToastWidget<'a> {
        ToastWidget::new(toasts, ActiveTheme::new(theme, ColorDepth::TrueColor))
    }

    fn painted(toasts: &[Toast], size: (u16, u16)) -> String {
        let theme = noir();
        rendered(size.0, size.1, |frame| {
            frame.render_widget(toaster(toasts, &theme), frame.area());
        })
        .to_string()
    }

    #[test]
    fn a_toast_stack_shows_the_newest_on_top() {
        let toasts = [
            Toast::info("Saved").with_text("The playlist was written."),
            Toast::error("Scan failed"),
            Toast::info("Hello"),
        ];
        insta::assert_snapshot!(painted(&toasts, (60, 20)));
    }

    #[test]
    fn a_toast_wraps_its_text() {
        let toasts = [Toast::info("Careful").with_text(
            "The quick brown fox jumps over the lazy dog and keeps running far away",
        )];
        insta::assert_snapshot!(painted(&toasts, (60, 12)));
    }

    #[test]
    fn each_kind_has_its_own_accent_and_icon() {
        let theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        assert_ne!(
            accent(&theme, ToastLevel::Info),
            accent(&theme, ToastLevel::Error)
        );
        assert_ne!(icon(ToastLevel::Info), icon(ToastLevel::Error));
    }

    #[test]
    fn a_minimal_screen_gets_one_plain_line_for_the_newest() {
        let toasts = [Toast::info("new"), Toast::info("old")];
        let theme = noir();
        let area = toaster(&toasts, &theme)
            .area(Rect::new(0, 0, 30, 6), Breakpoint::Minimal)
            .unwrap();
        assert_eq!((area.y, area.height), (0, 1));
        assert_eq!(area.right(), 30);
    }
}
