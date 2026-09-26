use std::fmt;

use config::{CoverStyle, TextCoverCells};
use num_traits::ToPrimitive;
use raster::{cover_aspect_ratio, round_u32};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cells(u16);

impl Cells {
    #[must_use]
    pub fn get(self) -> u16 {
        self.0
    }

    #[must_use]
    pub fn saturating_add(self, rhs: Cells) -> Cells {
        Cells(self.0.saturating_add(rhs.0))
    }

    #[must_use]
    pub fn saturating_sub(self, rhs: Cells) -> Cells {
        Cells(self.0.saturating_sub(rhs.0))
    }

    #[must_use]
    pub fn to_pixels(self, cell_px: Pixels) -> Pixels {
        Pixels(u32::from(self.0).saturating_mul(cell_px.0))
    }

    #[must_use]
    pub fn from_f32_floor(value: f32) -> Cells {
        Cells(clamp_floor_u16(value))
    }

    #[must_use]
    pub fn from_f32_round(value: f32) -> Cells {
        let rounded = value.round();
        Cells(clamp_floor_u16(rounded))
    }
}

impl fmt::Display for Cells {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for Cells {
    fn from(value: u16) -> Self {
        Cells(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pixels(u32);

impl Pixels {
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    pub fn saturating_add(self, rhs: Pixels) -> Pixels {
        Pixels(self.0.saturating_add(rhs.0))
    }

    #[must_use]
    pub fn saturating_sub(self, rhs: Pixels) -> Pixels {
        Pixels(self.0.saturating_sub(rhs.0))
    }

    #[must_use]
    pub fn to_cells(self, cell_px: Pixels) -> Cells {
        let cell = cell_px.0.max(1);
        let ratio = (self.0 / cell).to_f32().unwrap_or(f32::MAX);
        Cells(clamp_floor_u16(ratio))
    }

    #[must_use]
    pub fn from_f32_floor(value: f32) -> Pixels {
        Pixels(clamp_floor_u32(value))
    }

    #[must_use]
    pub fn from_f32_round(value: f32) -> Pixels {
        Pixels(round_u32(value))
    }
}

impl fmt::Display for Pixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for Pixels {
    fn from(value: u32) -> Self {
        Pixels(value)
    }
}

fn clamp_floor_u16(value: f32) -> u16 {
    let floored = value.floor();
    if floored.is_nan() {
        return 0;
    }
    if floored <= 0.0 {
        0
    } else {
        floored.to_u16().unwrap_or(u16::MAX)
    }
}

fn clamp_floor_u32(value: f32) -> u32 {
    let floored = value.floor();
    if floored.is_nan() {
        return 0;
    }
    if floored <= 0.0 {
        0
    } else {
        floored.to_u32().unwrap_or(u32::MAX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellAspect(pub f32);

impl Default for CellAspect {
    fn default() -> Self {
        CellAspect(2.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverAspect(pub f32);

impl Default for CoverAspect {
    fn default() -> Self {
        CoverAspect(1.0)
    }
}

impl From<f32> for CoverAspect {
    fn from(ratio: f32) -> Self {
        CoverAspect(ratio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Aspects {
    pub(crate) cell: CellAspect,
    pub(crate) cover: CoverAspect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverSizing {
    Auto { cover_aspect: CoverAspect },
    Fixed { width: u16, height: u16 },
    Off,
}

impl Default for CoverSizing {
    fn default() -> Self {
        CoverSizing::Auto {
            cover_aspect: CoverAspect::default(),
        }
    }
}

#[must_use]
pub(crate) fn cover_sizing(style: CoverStyle, cells: TextCoverCells) -> CoverSizing {
    match style {
        CoverStyle::Off => CoverSizing::Off,
        CoverStyle::Milkdrop => CoverSizing::Fixed {
            width: cells.width,
            height: cells.height,
        },
        CoverStyle::Plain | CoverStyle::Vinyl => CoverSizing::Auto {
            cover_aspect: CoverAspect::from(cover_aspect_ratio(style)),
        },
    }
}

#[cfg(test)]
mod tests {
    use config::{CoverStyle, TextCoverCells};
    use rstest::rstest;

    use crate::geometry::{Cells, CoverSizing, Pixels, cover_sizing};

    #[rstest]
    #[case::exact(40, 10, 4)]
    #[case::with_a_remainder(45, 10, 4)]
    #[case::shorter_than_one_cell(9, 10, 0)]
    fn pixels_and_cells_convert_both_ways(
        #[case] pixels: u32,
        #[case] cell: u32,
        #[case] cells: u16,
    ) {
        assert_eq!(
            Pixels::from(pixels).to_cells(Pixels::from(cell)).get(),
            cells
        );
        assert_eq!(
            Cells::from(cells).to_pixels(Pixels::from(cell)).get(),
            u32::from(cells) * cell
        );
    }

    #[rstest]
    #[case::rounds_down(3.9, 3)]
    #[case::negative_clamps(-1.5, 0)]
    fn from_f32_floor_never_rounds_up_or_wraps(
        #[case] value: f32,
        #[case] expected: u32,
    ) {
        assert_eq!(u32::from(Cells::from_f32_floor(value).get()), expected);
        assert_eq!(Pixels::from_f32_floor(value).get(), expected);
    }

    #[test]
    fn arithmetic_saturates_at_both_ends() {
        assert_eq!(
            Cells::from(u16::MAX).saturating_add(Cells::from(1)).get(),
            u16::MAX
        );
        assert_eq!(Cells::default().saturating_sub(Cells::from(1)).get(), 0);
    }

    #[test]
    fn display_is_the_bare_count() {
        assert_eq!(Cells::from(7_u16).to_string(), "7");
        assert_eq!(Pixels::from(7_u32).to_string(), "7");
    }

    #[test]
    fn off_stays_off() {
        let cells = TextCoverCells {
            width: 20,
            height: 8,
        };
        assert_eq!(cover_sizing(CoverStyle::Off, cells), CoverSizing::Off);
    }

    #[test]
    fn milkdrop_takes_a_fixed_text_grid() {
        let cells = TextCoverCells {
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
        let cells = TextCoverCells {
            width: 20,
            height: 8,
        };
        let sizing = cover_sizing(style, cells);
        assert!(matches!(sizing, CoverSizing::Auto { .. }));
    }
}
