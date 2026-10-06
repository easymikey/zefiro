use ratatui::{style::Color, text::Line};

use crate::{
    primitive::{
        glyphs::TITLE_SEPARATOR,
        span::{line, text},
    },
    theme::colors::Colors,
};

pub(crate) const COLUMN_SPACING: u16 = 1;
pub(crate) const QUERY_ROWS: u16 = 2;

#[must_use]
pub(crate) fn modal_title(
    word: &str,
    detail: String,
    colors: Colors<Color>,
) -> Line<'static> {
    line([
        text(format!("{word}{TITLE_SEPARATOR}")).fg(colors.muted_foreground),
        text(detail).fg(colors.muted_foreground),
    ])
}
