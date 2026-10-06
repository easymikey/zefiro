use unicode_width::UnicodeWidthStr;

use crate::primitive::glyphs;

pub(crate) const FAVORITE_COLUMNS: u16 = 2;
pub(crate) const PLAYING_COLUMNS: u16 = 2;
pub(crate) const MARKERS_WIDTH: u16 = FAVORITE_COLUMNS + PLAYING_COLUMNS;

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueNumber(usize);

impl QueueNumber {
    #[must_use]
    pub(crate) const fn new(queue_number: usize) -> Self {
        Self(queue_number)
    }

    pub(crate) fn chip(self) -> impl Iterator<Item = &'static str> + Clone {
        [glyphs::chip::OPEN, glyphs::playlist::QUEUED]
            .into_iter()
            .chain(self.digits())
            .chain([glyphs::chip::CLOSE])
    }

    fn digits(self) -> impl Iterator<Item = &'static str> + Clone {
        let places = self.0.checked_ilog10().map_or(1, |top| top + 1);
        (0..places).rev().map(move |place| {
            let digit = self.0 / 10_usize.pow(place) % 10;
            DIGITS.get(digit).map_or("", |&text| text)
        })
    }
}

#[must_use]
pub(crate) fn column_padding(glyph: &str, width: usize) -> usize {
    width.saturating_sub(glyph.width())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::primitive::marker::QueueNumber;

    #[rstest]
    #[case::single(1, "1")]
    #[case::ten(10, "10")]
    #[case::many(1203, "1203")]
    fn a_position_reads_as_its_decimal_digits(
        #[case] number: usize,
        #[case] text: &str,
    ) {
        let digits: String = QueueNumber::new(number).digits().collect();
        assert_eq!(digits, text);
    }
}
