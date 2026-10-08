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
pub(crate) fn cover_sizing(
    cover_mode: CoverMode,
    cover_cells: CoverCells,
) -> CoverSizing {
    match cover_mode {
        CoverMode::Off => CoverSizing::Off,
        CoverMode::Milkdrop => CoverSizing::Fixed {
            width: cover_cells.width,
            height: cover_cells.height,
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

    #[rstest]
    #[case::a_plain_cover_is_square(CoverMode::Plain, CoverSizing::Auto(1.0))]
    #[case::a_vinyl_cover_uses_the_vinyl_canvas_aspect_ratio(
        CoverMode::Vinyl,
        CoverSizing::Auto(canvas_aspect_ratio())
    )]
    #[case::off_stays_off(CoverMode::Off, CoverSizing::Off)]
    #[case::milkdrop_takes_a_fixed_text_grid(
        CoverMode::Milkdrop,
        CoverSizing::Fixed {
            width: Cells(24),
            height: Cells(9)
        }
    )]
    fn cover_sizing_follows_the_cover_mode(
        #[case] cover_mode: CoverMode,
        #[case] sizing: CoverSizing,
    ) {
        let cover_cells = CoverCells {
            width: Cells(24),
            height: Cells(9),
        };
        assert_eq!(cover_sizing(cover_mode, cover_cells), sizing);
    }
}
