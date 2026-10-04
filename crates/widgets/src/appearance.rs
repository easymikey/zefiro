use kernel::domain::{appearance::Rgb, geometry::Pixels};

use crate::{geometry::CoverCells, screen::breakpoint::Breakpoints};

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Appearance {
    pub cover_cells: CoverCells,
    pub breakpoints: Breakpoints,
    pub progress: ProgressBar,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBar {
    pub height: Pixels,
    pub radius: Option<Pixels>,
    pub fill: Option<Rgb>,
    pub groove: Option<Rgb>,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            height: Pixels(4),
            radius: None,
            fill: None,
            groove: None,
        }
    }
}
