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
pub(crate) fn modal_title<'a>(
    word: &'a str,
    detail: &'a str,
    colors: Colors<Color>,
) -> Line<'a> {
    line([
        text(word).fg(colors.muted_foreground),
        text(TITLE_SEPARATOR).fg(colors.muted_foreground),
        text(detail).fg(colors.muted_foreground),
    ])
}
