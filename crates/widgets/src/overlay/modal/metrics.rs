use ratatui::{
    style::{Color, Style},
    text::Line,
};

use crate::{
    primitive::{
        glyphs::TITLE_SEPARATOR,
        span::{line, text},
    },
    theme::{ActiveTheme, Role},
};

pub(crate) const COLUMN_SPACING: u16 = 1;
pub(crate) const QUERY_ROWS: u16 = 2;
pub(crate) const SCROLL_PADDING: usize = 1;
pub(crate) const SCROLLBAR_INSET: u16 = 2;

#[must_use]
pub(crate) fn modal_title(
    word: &str,
    detail: String,
    theme: ActiveTheme<'_>,
) -> Line<'static> {
    line([
        text(format!("{word}{TITLE_SEPARATOR}")).fg(theme.role(Role::Frame)),
        text(detail).fg(theme.role(Role::Dim)),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ModalRowColors {
    pub(crate) text: Color,
    pub(crate) dim: Color,
    pub(crate) selected_text: Color,
    pub(crate) selected_background: Color,
    pub(crate) accent: Color,
}

impl ModalRowColors {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            text: theme.role(Role::Text),
            dim: theme.role(Role::Dim),
            selected_text: theme.role(Role::SelectionForeground),
            selected_background: theme.role(Role::SelectionBackground),
            accent: theme.role(Role::Accent),
        }
    }

    #[must_use]
    pub(crate) fn highlight(self) -> Style {
        Style::default()
            .fg(self.selected_text)
            .bg(self.selected_background)
    }
}
