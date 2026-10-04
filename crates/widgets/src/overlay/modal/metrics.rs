use ratatui::{
    style::{Color, Style},
    text::Line,
};

use crate::{
    primitive::{
        glyphs::TITLE_SEPARATOR,
        span::{line, text},
    },
    theme::{active_theme::ActiveTheme, colors::Role},
};

pub(crate) const COLUMN_SPACING: u16 = 1;
pub(crate) const QUERY_ROWS: u16 = 2;
pub(crate) const SCROLL_PADDING: usize = 1;
pub(crate) const SCROLLBAR_INSET: u16 = 2;

#[must_use]
pub(crate) fn modal_title(
    word: &str,
    detail: String,
    style: ModalRowStyle,
) -> Line<'static> {
    line([
        text(format!("{word}{TITLE_SEPARATOR}")).fg(style.border),
        text(detail).fg(style.muted_foreground),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ModalRowStyle {
    pub(crate) foreground: Color,
    pub(crate) muted_foreground: Color,
    pub(crate) selected_foreground: Color,
    pub(crate) selected_background: Color,
    pub(crate) accent: Color,
    pub(crate) border: Color,
    pub(crate) background: Color,
}

impl ModalRowStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            foreground: theme.role(Role::Text),
            muted_foreground: theme.role(Role::Dim),
            selected_foreground: theme.role(Role::SelectionForeground),
            selected_background: theme.role(Role::SelectionBackground),
            accent: theme.role(Role::Accent),
            border: theme.role(Role::Frame),
            background: theme.role(Role::WindowBackground),
        }
    }

    #[must_use]
    pub(crate) fn highlight(self) -> Style {
        Style::default()
            .fg(self.selected_foreground)
            .bg(self.selected_background)
    }
}
