use ratatui::{
    style::{Color, Style},
    text::Line,
};

use crate::{
    primitive::{
        glyphs::TITLE_SEPARATOR,
        span::{line, text},
    },
    theme::ActiveTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalMetrics {
    pub(crate) column_spacing: u16,
    pub(crate) query_rows: u16,
    pub(crate) scroll_padding: usize,
    pub(crate) scrollbar_inset: u16,
}

impl Default for ModalMetrics {
    fn default() -> Self {
        Self {
            column_spacing: 1,
            query_rows: 2,
            scroll_padding: 1,
            scrollbar_inset: 2,
        }
    }
}

#[must_use]
pub(crate) fn modal_title(
    word: &str,
    detail: String,
    theme: ActiveTheme<'_>,
) -> Line<'static> {
    line([
        text(format!("{word}{TITLE_SEPARATOR}")).fg(theme.border()),
        text(detail).fg(theme.dim()),
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
            text: theme.text(),
            dim: theme.dim(),
            selected_text: theme.selection_foreground(),
            selected_background: theme.selection_background(),
            accent: theme.accent(),
        }
    }

    #[must_use]
    pub(crate) fn highlight(self) -> Style {
        Style::default()
            .fg(self.selected_text)
            .bg(self.selected_background)
    }
}
