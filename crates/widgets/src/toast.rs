use kernel::domain::{Moment, TOAST_LIFETIME, Toast, ToastKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{canvas::Canvas, inset::Inset, text::truncate},
    scene::Scene,
    screen::Breakpoint,
    theme::{ActiveTheme, Role},
};

const TOAST_WIDTH: u16 = 42;
const GAP: u16 = 1;
const INSET_CELLS: u16 = 1;
const CHROME_CELLS: u16 = 4;
const BORDER_ROWS: u16 = 2;
const TITLE_ROWS: u16 = 1;
const MAX_TEXT_ROWS: usize = 3;
const WARNING_HEAT: f32 = 0.75;

const CARD_INSET: Inset = Inset {
    top: 0,
    left: 1,
    right: 1,
    bottom: 0,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToastStyle {
    info: Color,
    success: Color,
    warning: Color,
    error: Color,
    text: Color,
    background: Color,
}

impl ToastStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            info: theme.role(Role::Accent),
            success: theme.spectrum_color_at(0.0),
            warning: theme.spectrum_color_at(WARNING_HEAT),
            error: theme.alert(),
            text: theme.role(Role::Text),
            background: theme.role(Role::WindowBackground),
        }
    }

    #[must_use]
    pub(crate) fn accent(&self, kind: ToastKind) -> Color {
        match kind {
            ToastKind::Info => self.info,
            ToastKind::Success => self.success,
            ToastKind::Warning => self.warning,
            ToastKind::Error => self.error,
        }
    }
}

fn icon(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Info => "i",
        ToastKind::Success => "\u{2713}",
        ToastKind::Warning => "!",
        ToastKind::Error => "\u{2717}",
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToastWidget<'a> {
    pub(crate) toasts: &'a [Toast],
    pub(crate) now: Moment,
    pub(crate) style: ToastStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastAreas {
    pub outer: Rect,
    pub painted: Rect,
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

    fn painted(areas: ToastAreas) -> Self {
        if areas.outer.height == 1 {
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
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.width() + 1 + word.width() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
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
    let mut shown: Vec<String> = lines.iter().take(head).map(clipped).collect();
    shown.push(truncate(&tail, width).into_owned());
    shown
}

fn title_line(toast: &Toast, room: usize) -> String {
    let line = format!("{} {}", icon(toast.kind), toast.title);
    truncate(&line, room).into_owned()
}

impl<'a> ToastWidget<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Option<Self> {
        (!scene.toasts.is_empty()).then(|| Self {
            toasts: scene.toasts,
            now: scene.now,
            style: ToastStyle::from_theme(&scene.active_theme()),
        })
    }

    fn live(&self) -> impl Iterator<Item = &'a Toast> + use<'a> {
        let now = self.now;
        self.toasts
            .iter()
            .filter(move |toast| now.elapsed_since(toast.raised_at) < TOAST_LIFETIME)
    }

    fn placed(&self, screen: Rect, form: Form) -> Vec<Placed<'a>> {
        match form {
            Form::Stack => self.stacked(screen),
            Form::Line => self.line(screen),
        }
    }

    fn line(&self, screen: Rect) -> Vec<Placed<'a>> {
        self.live()
            .next()
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
        let mut top = screen.y.saturating_add(INSET_CELLS);
        let mut placed = Vec::new();
        for toast in self.live() {
            let text = fitted(
                toast.text.as_deref().map_or("", str::trim),
                usize::from(room),
                MAX_TEXT_ROWS,
            );
            let rows = u16::try_from(text.len()).unwrap_or(0);
            let height = BORDER_ROWS + TITLE_ROWS + rows;
            if top.saturating_add(height) > screen.bottom() {
                break;
            }
            let mut lines = vec![title_line(toast, usize::from(room))];
            lines.extend(text);
            placed.push(Placed {
                toast,
                rect: Rect::new(x, top, width, height),
                lines,
            });
            top = top.saturating_add(height + GAP);
        }
        placed
    }

    #[must_use]
    pub(crate) fn areas(
        self,
        screen: Rect,
        breakpoint: Breakpoint,
    ) -> Option<ToastAreas> {
        let outer = self
            .placed(screen, Form::of(breakpoint))
            .iter()
            .map(|placed| placed.rect)
            .reduce(Rect::union)?;
        Some(ToastAreas {
            outer,
            painted: outer,
        })
    }

    pub(crate) fn paint(self, areas: ToastAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        for placed in self.placed(area, Form::painted(areas)) {
            self.paint_toast(&placed, buffer);
        }
    }

    fn paint_toast(&self, placed: &Placed<'_>, buffer: &mut Buffer) {
        let accent = self.style.accent(placed.toast.kind);
        let rect = placed.rect;
        Clear.render(rect, buffer);
        Block::new()
            .style(
                Style::default()
                    .bg(self.style.background)
                    .fg(self.style.text),
            )
            .render(rect, buffer);
        let mut lines = placed.lines.iter();
        let title = Paragraph::new(lines.next().map_or("", String::as_str))
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
        let body = lines.cloned().collect::<Vec<_>>().join("\n");
        let below = Rect {
            y: inner.y.saturating_add(1),
            height: inner.height.saturating_sub(1),
            ..inner
        };
        Paragraph::new(body)
            .style(Style::default().fg(self.style.text))
            .render(below, buffer);
    }
}

impl Widget for ToastWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(areas) = self.areas(area, Breakpoint::Full) {
            self.paint(areas, Canvas { area, buffer });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{Moment, Toast, ToastKind};
    use ratatui::layout::Rect;

    use crate::{
        screen::Breakpoint,
        test_support::{noir, rendered},
        theme::{ActiveTheme, ColorDepth},
        toast::{ToastStyle, ToastWidget, icon},
    };

    fn style() -> ToastStyle {
        let theme = noir();
        ToastStyle::from_theme(&ActiveTheme::new(&theme, ColorDepth::TrueColor))
    }

    fn toaster(toasts: &[Toast]) -> ToastWidget<'_> {
        ToastWidget {
            toasts,
            now: Moment::default(),
            style: style(),
        }
    }

    fn painted(toasts: &[Toast], size: (u16, u16)) -> String {
        rendered(size.0, size.1, |frame| {
            frame.render_widget(toaster(toasts), frame.area());
        })
        .to_string()
    }

    #[test]
    fn a_toast_stack_shows_the_newest_on_top() {
        let toasts = [
            Toast::success("Saved").with_text("The playlist was written."),
            Toast::error("Scan failed"),
            Toast::info("Hello"),
        ];
        insta::assert_snapshot!(painted(&toasts, (60, 20)));
    }

    #[test]
    fn a_toast_wraps_its_text() {
        let toasts = [Toast::warning("Careful").with_text(
            "The quick brown fox jumps over the lazy dog and keeps running far away",
        )];
        insta::assert_snapshot!(painted(&toasts, (60, 12)));
    }

    #[test]
    fn each_kind_has_its_own_accent_and_icon() {
        let style = style();
        let kinds = [
            ToastKind::Info,
            ToastKind::Success,
            ToastKind::Warning,
            ToastKind::Error,
        ];
        for (index, kind) in kinds.iter().enumerate() {
            for other in kinds.iter().skip(index + 1) {
                assert_ne!(style.accent(*kind), style.accent(*other));
                assert_ne!(icon(*kind), icon(*other));
            }
        }
    }

    #[test]
    fn a_minimal_screen_gets_one_plain_line_for_the_newest() {
        let toasts = [Toast::info("new"), Toast::info("old")];
        let areas = toaster(&toasts)
            .areas(Rect::new(0, 0, 30, 6), Breakpoint::Minimal)
            .unwrap();
        assert_eq!((areas.outer.y, areas.outer.height), (0, 1));
        assert_eq!(areas.outer.right(), 30);
    }

    #[test]
    fn an_expired_toast_is_not_painted() {
        let toasts = [Toast::info("old")];
        let later = ToastWidget {
            now: Moment::new(Duration::from_secs(9)),
            ..toaster(&toasts)
        };
        assert_eq!(later.areas(Rect::new(0, 0, 60, 20), Breakpoint::Full), None);
    }
}
