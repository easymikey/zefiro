use unicode_width::UnicodeWidthStr;

use crate::{Playing, primitive::glyphs};

pub(crate) const FAVORITE_COLUMNS: u16 = 2;
pub(crate) const PLAYING_COLUMNS: u16 = 2;
pub(crate) const MARKERS_WIDTH: u16 = FAVORITE_COLUMNS + PLAYING_COLUMNS;

#[must_use]
pub(crate) fn favorite_marker(favorite: Favorite) -> &'static str {
    match favorite {
        Favorite::Yes => glyphs::playlist::FAVORITE,
        Favorite::No => "",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Favorite {
    Yes,
    No,
}

#[must_use]
pub(crate) fn playing_marker(playing: Playing) -> &'static str {
    match playing {
        Playing::Yes => glyphs::playlist::PLAYING,
        Playing::No => "",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueuePosition(usize);

impl QueuePosition {
    #[must_use]
    pub(crate) const fn new(position: usize) -> Self {
        Self(position)
    }

    #[must_use]
    pub(crate) fn label(self) -> String {
        format!("{}{}", glyphs::playlist::QUEUED, self.0)
    }
}

#[must_use]
pub(crate) fn column_padding(glyph: &str, width: usize) -> usize {
    width.saturating_sub(glyph.width())
}
