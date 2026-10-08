use std::sync::Arc;

use kernel::domain::toast::{Toast, ToastLevel};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{inset::Inset, truncate::truncate_owned},
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
pub(crate) fn accent(active_theme: &ActiveTheme<'_>, level: ToastLevel) -> Color {
    match level {
        ToastLevel::Info => active_theme.colors().accent,
        ToastLevel::Error => active_theme.alert(),
    }
}

fn icon(level: ToastLevel) -> &'static str {
    match level {
        ToastLevel::Info => "i",
        ToastLevel::Error => "\u{2717}",
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToastWidget<'a> {
    toasts: &'a [Toast],
    active_theme: ActiveTheme<'a>,
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
}

#[derive(Debug, PartialEq)]
struct PlacedToast<'a> {
    toast: &'a Toast,
    rect: Rect,
    form: Form,
    title: String,
    body_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placement<'a> {
    area: Rect,
    placed_toasts: Arc<[PlacedToast<'a>]>,
}

impl Placement<'_> {
    #[must_use]
    pub fn area(&self) -> Rect {
        self.area
    }
}

fn wrapped_paragraph(text: &str, width: usize) -> Vec<String> {
    text.split_whitespace()
        .fold(Vec::new(), |mut lines: Vec<String>, word| {
            if let Some(line) = lines.last_mut()
                && line.width() + 1 + word.width() <= width
            {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(word.to_owned());
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
    let lines = wrapped(text, width);
    if lines.len() <= rows {
        return lines
            .into_iter()
            .map(|line| truncate_owned(line, width))
            .collect();
    }
    let head = rows.saturating_sub(1);
    let tail = lines
        .get(head..)
        .map_or_else(String::new, |rest| rest.join(" "));
    lines
        .into_iter()
        .take(head)
        .chain(std::iter::once(tail))
        .map(|line| truncate_owned(line, width))
        .collect()
}

fn title_text(toast: &Toast, room: usize) -> String {
    let line = format!("{} {}", icon(toast.level), toast.title);
    truncate_owned(line, room)
}

impl<'a> ToastWidget<'a> {
    #[must_use]
    pub(crate) fn new(toasts: &'a [Toast], active_theme: ActiveTheme<'a>) -> Self {
        Self {
            toasts,
            active_theme,
        }
    }

    fn placed(&self, screen: Rect, form: Form) -> Vec<PlacedToast<'a>> {
        match form {
            Form::Stack => self.stacked(screen),
            Form::Line => self.line(screen).into_iter().collect(),
        }
    }

    fn line(&self, screen: Rect) -> Option<PlacedToast<'a>> {
        self.toasts.first().map(|toast| {
            let title = title_text(toast, usize::from(screen.width));
            let width = u16::try_from(title.width()).unwrap_or(screen.width);
            PlacedToast {
                toast,
                rect: Rect {
                    x: screen.right().saturating_sub(width),
                    y: screen.y,
                    width,
                    height: 1,
                },
                form: Form::Line,
                title,
                body_lines: Vec::new(),
            }
        })
    }

    fn stacked(&self, screen: Rect) -> Vec<PlacedToast<'a>> {
        let width = TOAST_WIDTH.min(screen.width.saturating_sub(INSET_CELLS * 2));
        let Some(room) = width.checked_sub(CHROME_CELLS).filter(|room| *room > 0)
        else {
            return Vec::new();
        };
        let x = screen.right().saturating_sub(INSET_CELLS + width);
        self.toasts
            .iter()
            .scan(screen.y.saturating_add(INSET_CELLS), |top, toast| {
                let body_lines = fitted(
                    toast.text.as_deref().map_or("", str::trim),
                    usize::from(room),
                    MAX_TEXT_ROWS,
                );
                let height = BORDER_ROWS
                    + TITLE_ROWS
                    + u16::try_from(body_lines.len()).unwrap_or(0);
                (top.saturating_add(height) <= screen.bottom()).then(|| {
                    let rect = Rect::new(x, *top, width, height);
                    *top = top.saturating_add(height + GAP);
                    PlacedToast {
                        toast,
                        rect,
                        form: Form::Stack,
                        title: title_text(toast, usize::from(room)),
                        body_lines,
                    }
                })
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn area(
        self,
        screen: Rect,
        breakpoint: Breakpoint,
    ) -> Option<Placement<'a>> {
        let placed_toasts: Arc<[PlacedToast<'a>]> =
            self.placed(screen, Form::of(breakpoint)).into();
        let area = placed_toasts
            .iter()
            .map(|placed_toast| placed_toast.rect)
            .reduce(Rect::union)?;
        Some(Placement {
            area,
            placed_toasts,
        })
    }

    pub(crate) fn paint(self, placement: &Placement<'_>, buffer: &mut Buffer) {
        for placed_toast in placement.placed_toasts.iter() {
            self.paint_toast(placed_toast, buffer);
        }
    }

    fn paint_toast(&self, placed_toast: &PlacedToast<'_>, buffer: &mut Buffer) {
        let PlacedToast {
            toast,
            rect,
            form,
            ref title,
            ref body_lines,
        } = *placed_toast;
        let accent = accent(&self.active_theme, toast.level);
        let colors = self.active_theme.colors();
        Clear.render(rect, buffer);
        Block::new()
            .style(
                Style::default()
                    .bg(colors.window_background)
                    .fg(colors.foreground),
            )
            .render(rect, buffer);
        let title_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
        match form {
            Form::Line => {
                Paragraph::new(title.as_str())
                    .style(title_style)
                    .render(rect, buffer);
            }
            Form::Stack => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(accent))
                    .padding(CARD_INSET.padding());
                let inner = block.inner(rect);
                block.render(rect, buffer);
                Paragraph::new(title.as_str())
                    .style(title_style)
                    .render(Rect { height: 1, ..inner }, buffer);
                let below = Rect {
                    y: inner.y.saturating_add(1),
                    height: inner.height.saturating_sub(1),
                    ..inner
                };
                Paragraph::new(
                    body_lines
                        .iter()
                        .map(|line| Line::from(line.as_str()))
                        .collect::<Vec<_>>(),
                )
                .style(Style::default().fg(colors.foreground))
                .render(below, buffer);
            }
        }
    }
}

impl Widget for ToastWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(placement) = self.area(area, Breakpoint::Full) {
            self.paint(&placement, buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::toast::{Toast, ToastLevel};
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Modifier},
    };
    use rstest::rstest;

    use crate::{
        screen::breakpoint::Breakpoint,
        test_support::{noir, rendered},
        theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
        toast::{ToastWidget, accent, icon},
    };

    fn toast_widget<'a>(toasts: &'a [Toast], theme: &'a Theme) -> ToastWidget<'a> {
        ToastWidget::new(toasts, ActiveTheme::new(theme, ColorDepth::TrueColor))
    }

    fn painted(toasts: &[Toast], size: (u16, u16)) -> String {
        let theme = noir();
        rendered(size.0, size.1, |frame| {
            frame.render_widget(toast_widget(toasts, &theme), frame.area());
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
    fn each_level_has_its_own_accent_and_icon() {
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        assert_ne!(
            accent(&active_theme, ToastLevel::Info),
            accent(&active_theme, ToastLevel::Error)
        );
        assert_ne!(icon(ToastLevel::Info), icon(ToastLevel::Error));
    }

    #[test]
    fn a_minimal_screen_gets_one_plain_line_for_the_newest() {
        let toasts = [Toast::info("new"), Toast::info("old")];
        let theme = noir();
        let area = toast_widget(&toasts, &theme)
            .area(Rect::new(0, 0, 30, 6), Breakpoint::Minimal)
            .unwrap()
            .area();
        assert_eq!((area.y, area.height), (0, 1));
        assert_eq!(area.right(), 30);
    }

    #[rstest]
    #[case::wide(60, Rect::new(17, 1, 42, 12))]
    #[case::narrow(30, Rect::new(1, 1, 28, 13))]
    fn the_area_is_the_union_of_the_painted_toasts(
        #[case] screen_width: u16,
        #[case] expected: Rect,
    ) {
        let toasts = [
            Toast::info("Careful").with_text(
                "The quick brown fox jumps over the lazy dog and keeps running far away",
            ),
            Toast::error("Scan failed").with_text("line one\nline two\nline three\nline four"),
        ];
        let theme = noir();
        let placement = toast_widget(&toasts, &theme)
            .area(Rect::new(0, 0, screen_width, 20), Breakpoint::Full)
            .unwrap();
        let painted = placement
            .placed_toasts
            .iter()
            .map(|placed_toast| placed_toast.rect)
            .reduce(Rect::union);
        assert_eq!(Some(placement.area()), painted);
        assert_eq!(placement.area(), expected);
    }

    fn alert(active_theme: &ActiveTheme<'_>) -> Color {
        accent(active_theme, ToastLevel::Error)
    }

    fn foreground(active_theme: &ActiveTheme<'_>) -> Color {
        active_theme.colors().foreground
    }

    #[rstest]
    #[case::title((19, 2), alert, Modifier::BOLD)]
    #[case::body((19, 3), foreground, Modifier::empty())]
    #[case::bottom_border((19, 4), alert, Modifier::empty())]
    fn a_card_paints_its_title_bold_and_its_body_inside_the_border(
        #[case] position: (u16, u16),
        #[case] fg: fn(&ActiveTheme<'_>) -> Color,
        #[case] modifier: Modifier,
    ) {
        let toasts = [Toast::error("Scan failed").with_text("line one")];
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let widget = toast_widget(&toasts, &theme);
        let screen = Rect::new(0, 0, 60, 20);
        let placement = widget.area(screen, Breakpoint::Full).unwrap();
        assert_eq!(placement.area(), Rect::new(17, 1, 42, 4));
        let mut buffer = Buffer::empty(screen);
        widget.paint(&placement, &mut buffer);
        let cell = &buffer[position];
        assert_eq!((cell.fg, cell.modifier), (fg(&active_theme), modifier));
    }
}
