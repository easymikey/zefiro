use crate::{
    geometry::CoverCells,
    primitive::bar::ProgressBar,
    screen::breakpoint::Breakpoints,
};

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Appearance {
    pub cover_cells: CoverCells,
    pub breakpoints: Breakpoints,
    pub progress: ProgressBar,
}
