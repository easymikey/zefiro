use unicode_width::UnicodeWidthStr;

use crate::{Playing, primitive::glyphs::PlaylistGlyphs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkerColumns {
    pub favorite: u16,
    pub playing: u16,
}

impl Default for MarkerColumns {
    fn default() -> Self {
        Self {
            favorite: 2,
            playing: 2,
        }
    }
}

impl MarkerColumns {
    #[must_use]
    pub(crate) const fn total(self) -> u16 {
        self.favorite + self.playing
    }
}

#[must_use]
pub(crate) fn favorite_marker(
    favorite: Favorite,
    glyphs: PlaylistGlyphs,
) -> &'static str {
    match favorite {
        Favorite::Yes => glyphs.favorite,
        Favorite::No => "",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Favorite {
    Yes,
    No,
}

#[must_use]
pub(crate) fn playing_marker(playing: Playing, glyphs: PlaylistGlyphs) -> &'static str {
    match playing {
        Playing::Yes => glyphs.playing,
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
    pub(crate) fn label(self, glyphs: PlaylistGlyphs) -> String {
        format!("{}{}", glyphs.queued, self.0)
    }
}

#[must_use]
pub(crate) fn column_padding(glyph: &str, width: usize) -> usize {
    width.saturating_sub(glyph.width())
}
