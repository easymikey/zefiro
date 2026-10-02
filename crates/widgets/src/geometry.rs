use kernel::domain::appearance::{CoverCells, CoverStyle};

use crate::pixels::canvas_aspect_ratio;

pub const DEFAULT_CELL_ASPECT: f32 = 2.0;

const SQUARE_COVER_ASPECT: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverSizing {
    Auto { cover_aspect: f32 },
    Fixed { width: u16, height: u16 },
    Off,
}

impl Default for CoverSizing {
    fn default() -> Self {
        CoverSizing::Auto {
            cover_aspect: SQUARE_COVER_ASPECT,
        }
    }
}

#[must_use]
pub(crate) fn cover_sizing(style: CoverStyle, cells: CoverCells) -> CoverSizing {
    match style {
        CoverStyle::Off => CoverSizing::Off,
        CoverStyle::Milkdrop => CoverSizing::Fixed {
            width: cells.width,
            height: cells.height,
        },
        CoverStyle::Plain => CoverSizing::Auto {
            cover_aspect: SQUARE_COVER_ASPECT,
        },
        CoverStyle::Vinyl => CoverSizing::Auto {
            cover_aspect: canvas_aspect_ratio(),
        },
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::{CoverCells, CoverStyle};
    use rstest::rstest;

    use crate::geometry::{CoverSizing, canvas_aspect_ratio, cover_sizing};

    #[test]
    fn a_plain_cover_is_square() {
        let cells = CoverCells {
            width: 20,
            height: 8,
        };
        assert_eq!(
            cover_sizing(CoverStyle::Plain, cells),
            CoverSizing::Auto { cover_aspect: 1.0 }
        );
    }

    #[test]
    fn a_vinyl_cover_uses_the_vinyl_canvas_aspect_ratio() {
        let cells = CoverCells {
            width: 20,
            height: 8,
        };
        assert_eq!(
            cover_sizing(CoverStyle::Vinyl, cells),
            CoverSizing::Auto {
                cover_aspect: canvas_aspect_ratio()
            }
        );
    }

    #[test]
    fn off_stays_off() {
        let cells = CoverCells {
            width: 20,
            height: 8,
        };
        assert_eq!(cover_sizing(CoverStyle::Off, cells), CoverSizing::Off);
    }

    #[test]
    fn milkdrop_takes_a_fixed_text_grid() {
        let cells = CoverCells {
            width: 24,
            height: 9,
        };
        let sizing = cover_sizing(CoverStyle::Milkdrop, cells);
        assert_eq!(
            sizing,
            CoverSizing::Fixed {
                width: 24,
                height: 9
            }
        );
    }

    #[rstest]
    #[case::plain(CoverStyle::Plain)]
    #[case::vinyl(CoverStyle::Vinyl)]
    fn plain_and_vinyl_size_themselves_from_their_own_aspect_ratio(
        #[case] style: CoverStyle,
    ) {
        let cells = CoverCells {
            width: 20,
            height: 8,
        };
        let sizing = cover_sizing(style, cells);
        assert!(matches!(sizing, CoverSizing::Auto { .. }));
    }
}
