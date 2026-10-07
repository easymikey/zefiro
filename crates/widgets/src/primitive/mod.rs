use std::fmt::{self, Display, Write};

use unicode_width::UnicodeWidthStr;

pub mod bar;
pub(crate) mod canvas;
pub(crate) mod chip;
pub(crate) mod corner_brackets;
pub(crate) mod format_chips;
pub(crate) mod glyphs;
pub(crate) mod inset;
pub(crate) mod list_chrome;
pub(crate) mod marker;
pub(crate) mod span;
pub(crate) mod spectrum_meter;
pub(crate) mod time_text;
pub(crate) mod track_row;
pub(crate) mod truncate;

struct CharCount(usize);

impl Write for CharCount {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 += text.width();
        Ok(())
    }
}

#[must_use]
pub(crate) fn display_width(text: &impl Display) -> usize {
    let mut count = CharCount(0);
    write!(count, "{text}").map_or(0, |()| count.0)
}
