use kernel::domain::Toast;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    primitive::{
        canvas::Canvas,
        glyphs::TruncateGlyphs,
        inset::Inset,
        text::truncate_to_width,
    },
    theme::ActiveTheme,
};

const MARGIN: u16 = 0;
const CHROME_CELLS: u16 = 4;
const BORDER_ROWS: u16 = 2;
const MAX_ROWS: u16 = 3;
const WIDTH_SHARE_PERCENT: u32 = 40;

const CARD_INSET: Inset = Inset {
    top: 0,
    left: 1,
    right: 1,
    bottom: 0,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToastCard<'a> {
    pub(crate) toast: &'a Toast,
    pub(crate) theme: ActiveTheme<'a>,
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
    let glyphs = TruncateGlyphs::default();
    let clipped = |line: &String| truncate_to_width(line, width, glyphs).into_owned();
    let lines = wrapped(text, width);
    if lines.len() <= rows {
        return lines.iter().map(clipped).collect();
    }
    let head = rows.saturating_sub(1);
    let tail = lines
        .get(head..)
        .map(|rest| rest.join(" "))
        .unwrap_or_default();
    let mut shown: Vec<String> = lines.iter().take(head).map(clipped).collect();
    shown.push(truncate_to_width(&tail, width, glyphs).into_owned());
    shown
}

fn max_width(screen: Rect) -> u16 {
    let share = u16::try_from(u32::from(screen.width) * WIDTH_SHARE_PERCENT / 100)
        .unwrap_or(u16::MAX);
    share.min(screen.width.saturating_sub(MARGIN * 2))
}

fn card_width(lines: &[String]) -> u16 {
    let widest = lines.iter().map(|line| line.width()).max().unwrap_or(0);
    u16::try_from(widest)
        .unwrap_or(u16::MAX)
        .saturating_add(CHROME_CELLS)
}

fn top_right(screen: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: screen.right().saturating_sub(MARGIN + width),
        y: screen.y.saturating_add(MARGIN),
        width,
        height,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastAreas {
    pub outer: Rect,
    pub painted: Rect,
}

impl ToastCard<'_> {
    fn accent(self) -> Color {
        self.theme.accent()
    }

    fn block(self) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.accent()))
            .padding(CARD_INSET.padding())
    }

    fn lines(self, screen: Rect) -> Option<Vec<String>> {
        let text_room = max_width(screen)
            .checked_sub(CHROME_CELLS)
            .filter(|room| *room > 0)?;
        let rows = screen
            .height
            .saturating_sub(MARGIN + BORDER_ROWS)
            .min(MAX_ROWS);
        if rows == 0 {
            return None;
        }
        Some(fitted(
            self.toast.text.as_str(),
            usize::from(text_room),
            usize::from(rows),
        ))
    }

    #[must_use]
    pub(crate) fn areas(self, screen: Rect) -> Option<ToastAreas> {
        let lines = self.lines(screen)?;
        let width = card_width(&lines);
        let height = u16::try_from(lines.len())
            .unwrap_or(MAX_ROWS)
            .max(1)
            .saturating_add(BORDER_ROWS);
        let outer = top_right(screen, width, height);
        Some(ToastAreas {
            outer,
            painted: outer,
        })
    }

    pub(crate) fn render_in(self, areas: ToastAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        let Some(lines) = self.lines(area) else {
            return;
        };
        let outer = areas.outer;
        let window_background = self.theme.window_background();
        let block = self.block();
        let inner = block.inner(outer);
        Clear.render(outer, buffer);
        Block::new()
            .style(Style::default().bg(window_background).fg(self.accent()))
            .render(outer, buffer);
        block.render(outer, buffer);
        Paragraph::new(lines.join("\n"))
            .style(Style::default().fg(self.accent()).bg(window_background))
            .render(inner, buffer);
    }
}

impl Widget for &ToastCard<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if let Some(areas) = self.areas(area) {
            self.render_in(areas, Canvas { area, buffer });
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{Toast, ToastLevel};
    use ratatui::layout::Rect;

    use crate::{
        scene::fixtures::noir,
        theme::{ActiveTheme, ColorDepth},
        toast::ToastCard,
    };

    fn screen(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    #[test]
    fn a_short_toast_sits_in_the_top_right_corner() {
        let theme = noir();
        let toast = Toast {
            level: ToastLevel::Info,
            text: "Not in library".to_string(),
        };
        let card = ToastCard {
            toast: &toast,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        let area = screen(100, 30);
        let painted = card.areas(area).unwrap().painted;
        assert!(painted.right() <= area.right());
        assert_eq!(painted.y, 0);
    }

    #[test]
    fn a_long_toast_wraps_and_truncates_to_at_most_three_rows() {
        let theme = noir();
        let toast = Toast {
            level: ToastLevel::Error,
            text: "The output device changed while a track was playing, so playback \
                   paused until sound returns"
                .to_string(),
        };
        let card = ToastCard {
            toast: &toast,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        let painted = card.areas(screen(100, 30)).unwrap().painted;
        const BORDER_ROWS: u16 = 2;
        const MAX_ROWS: u16 = 3;
        assert!(painted.height <= MAX_ROWS + BORDER_ROWS);
    }

    #[test]
    fn a_toast_with_no_room_has_no_areas() {
        let theme = noir();
        let toast = Toast {
            level: ToastLevel::Info,
            text: "Not in library".to_string(),
        };
        let card = ToastCard {
            toast: &toast,
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
        };
        assert_eq!(card.areas(screen(3, 30)), None);
    }
}
