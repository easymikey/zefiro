use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect, Size},
    style::Style,
    widgets::{Paragraph, Widget},
};

use crate::{
    primitive::{
        span::{row, text},
        text::truncate,
    },
    theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TooSmallGlyphs {
    headline: &'static str,
    resize_prefix: &'static str,
    dimension_separator: &'static str,
    current_open: &'static str,
    current_close: &'static str,
}

impl Default for TooSmallGlyphs {
    fn default() -> Self {
        Self {
            headline: "Terminal too small.",
            resize_prefix: "Resize to at least ",
            dimension_separator: "\u{00d7}",
            current_open: "(now ",
            current_close: ")",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TooSmallNotice<'a> {
    pub theme: ActiveTheme<'a>,
    pub minimum: Size,
}

impl Widget for &TooSmallNotice<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let glyphs = TooSmallGlyphs::default();
        let text_style = Style::default().fg(self.theme.text());
        let dim_style = Style::default().fg(self.theme.dim());
        let width = usize::from(area.width);
        let fit = |line: String| truncate(&line, width).into_owned();
        let resize_line = fit(format!(
            "{}{}{}{}",
            glyphs.resize_prefix,
            self.minimum.width,
            glyphs.dimension_separator,
            self.minimum.height
        ));
        let current_line = fit(format!(
            "{}{}{}{}{}",
            glyphs.current_open,
            area.width,
            glyphs.dimension_separator,
            area.height,
            glyphs.current_close
        ));
        let lines = vec![
            row([text(fit(glyphs.headline.to_string())).style(text_style)])
                .alignment(Alignment::Center),
            row([text(resize_line).style(text_style)]).alignment(Alignment::Center),
            row([text(current_line).style(dim_style)]).alignment(Alignment::Center),
        ];
        let content_height = u16::try_from(lines.len()).unwrap_or(0).min(area.height);
        let target = Rect {
            y: area.y + area.height.saturating_sub(content_height) / 2,
            height: content_height,
            ..area
        };
        Paragraph::new(lines).render(target, buffer);
    }
}
