use kernel::domain::{
    appearance::{CoverCells, CoverMode},
    geometry::Cells,
};

use crate::pixels::vinyl::geometry::canvas_aspect_ratio;

pub const DEFAULT_CELL_ASPECT: f32 = 2.0;

const SQUARE_COVER_ASPECT: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CoverSizing {
    Auto(f32),
    Fixed { width: Cells, height: Cells },
    Off,
}

impl Default for CoverSizing {
    fn default() -> Self {
        CoverSizing::Auto(SQUARE_COVER_ASPECT)
    }
}

#[must_use]
pub(crate) fn cover_sizing(style: CoverMode, cells: CoverCells) -> CoverSizing {
    match style {
        CoverMode::Off => CoverSizing::Off,
        CoverMode::Milkdrop => CoverSizing::Fixed {
            width: cells.width,
            height: cells.height,
        },
        CoverMode::Plain => CoverSizing::Auto(SQUARE_COVER_ASPECT),
        CoverMode::Vinyl => CoverSizing::Auto(canvas_aspect_ratio()),
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::{CoverCells, CoverMode},
        geometry::Cells,
    };
    use rstest::rstest;

    use crate::geometry::{CoverSizing, canvas_aspect_ratio, cover_sizing};

    #[test]
    fn a_plain_cover_is_square() {
        let cells = CoverCells {
            width: Cells(20),
            height: Cells(8),
        };
        assert_eq!(
            cover_sizing(CoverMode::Plain, cells),
            CoverSizing::Auto(1.0)
        );
    }

    #[test]
    fn a_vinyl_cover_uses_the_vinyl_canvas_aspect_ratio() {
        let cells = CoverCells {
            width: Cells(20),
            height: Cells(8),
        };
        assert_eq!(
            cover_sizing(CoverMode::Vinyl, cells),
            CoverSizing::Auto(canvas_aspect_ratio())
        );
    }

    #[test]
    fn off_stays_off() {
        let cells = CoverCells {
            width: Cells(20),
            height: Cells(8),
        };
        assert_eq!(cover_sizing(CoverMode::Off, cells), CoverSizing::Off);
    }

    #[test]
    fn milkdrop_takes_a_fixed_text_grid() {
        let cells = CoverCells {
            width: Cells(24),
            height: Cells(9),
        };
        let sizing = cover_sizing(CoverMode::Milkdrop, cells);
        assert_eq!(
            sizing,
            CoverSizing::Fixed {
                width: Cells(24),
                height: Cells(9)
            }
        );
    }

    #[rstest]
    #[case::plain(CoverMode::Plain)]
    #[case::vinyl(CoverMode::Vinyl)]
    fn plain_and_vinyl_size_themselves_from_their_own_aspect_ratio(
        #[case] style: CoverMode,
    ) {
        let cells = CoverCells {
            width: Cells(20),
            height: Cells(8),
        };
        let sizing = cover_sizing(style, cells);
        assert!(matches!(sizing, CoverSizing::Auto(_)));
    }
}
