use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    widgets::{Paragraph, Widget},
};

use crate::{
    primitive::{
        span::{line, text},
        text::truncate,
    },
    theme::{active_theme::ActiveTheme, colors::Role},
};

const HEADLINE: &str = "Terminal too small.";
const RESIZE_PREFIX: &str = "Resize to at least ";
const DIMENSION_SEPARATOR: &str = "\u{00d7}";
const CURRENT_OPEN: &str = "(now ";
const CURRENT_CLOSE: &str = ")";

#[derive(Debug, Clone, Copy)]
pub(crate) struct TooSmallStyle {
    pub(crate) foreground: Color,
    pub(crate) muted_foreground: Color,
}

impl TooSmallStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            foreground: theme.role(Role::Text),
            muted_foreground: theme.role(Role::Dim),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TooSmallWidget<'a> {
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) minimum: Size,
}

impl Widget for &TooSmallWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let style = TooSmallStyle::from_theme(&self.theme);
        let text_style = Style::default().fg(style.foreground);
        let dim_style = Style::default().fg(style.muted_foreground);
        let width = usize::from(area.width);
        let fit = |line: String| truncate(&line, width).into_owned();
        let resize_line = fit(format!(
            "{}{}{}{}",
            RESIZE_PREFIX, self.minimum.width, DIMENSION_SEPARATOR, self.minimum.height
        ));
        let current_line = fit(format!(
            "{}{}{}{}{}",
            CURRENT_OPEN, area.width, DIMENSION_SEPARATOR, area.height, CURRENT_CLOSE
        ));
        let lines = vec![
            line([text(fit(HEADLINE.to_string())).style(text_style)])
                .alignment(Alignment::Center),
            line([text(resize_line).style(text_style)]).alignment(Alignment::Center),
            line([text(current_line).style(dim_style)]).alignment(Alignment::Center),
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
