use kernel::domain::appearance::{Breakpoints, LayoutMode};
use ratatui::layout::Size;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Full,
    Compact,
    Minimal,
    TooSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Fits,
    Short,
}

fn fit(size: Size, width: u16, height: u16) -> Fit {
    if size.width >= width && size.height >= height {
        Fit::Fits
    } else {
        Fit::Short
    }
}

impl Breakpoint {
    #[must_use]
    pub fn new(size: Size, layout: &Breakpoints, mode: LayoutMode) -> Self {
        if fit(size, layout.min_columns.0, layout.min_rows.0) == Fit::Short {
            return Self::TooSmall;
        }
        let full = fit(size, layout.full_min_width.0, layout.full_min_height.0);
        let compact = fit(
            size,
            layout.compact_min_width.0,
            layout.compact_min_height.0,
        );
        match (mode, full, compact) {
            (LayoutMode::Compact, _, Fit::Fits) => Self::Compact,
            (
                LayoutMode::Auto | LayoutMode::Full | LayoutMode::Compact,
                Fit::Fits,
                _,
            ) => Self::Full,
            (LayoutMode::Auto | LayoutMode::Full, Fit::Short, Fit::Fits) => {
                Self::Compact
            }
            (
                LayoutMode::Auto | LayoutMode::Full | LayoutMode::Compact,
                Fit::Short,
                Fit::Short,
            ) => Self::Minimal,
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::{Breakpoints, LayoutMode},
        geometry::Cells,
    };
    use ratatui::layout::Size;
    use rstest::rstest;

    use crate::screen::Breakpoint;

    #[rstest]
    #[case::wide(LayoutMode::Auto, Size::new(80, 24), Breakpoint::Full)]
    #[case::full_edge(LayoutMode::Auto, Size::new(60, 19), Breakpoint::Full)]
    #[case::short(LayoutMode::Auto, Size::new(80, 18), Breakpoint::Compact)]
    #[case::narrow(LayoutMode::Auto, Size::new(59, 24), Breakpoint::Compact)]
    #[case::compact_asked(LayoutMode::Compact, Size::new(80, 24), Breakpoint::Compact)]
    #[case::full_asked_but_short(
        LayoutMode::Full,
        Size::new(80, 18),
        Breakpoint::Compact
    )]
    #[case::below_minimum(LayoutMode::Auto, Size::new(47, 24), Breakpoint::TooSmall)]
    #[case::below_minimum_rows(
        LayoutMode::Auto,
        Size::new(80, 15),
        Breakpoint::TooSmall
    )]
    fn the_terminal_size_picks_the_breakpoint(
        #[case] mode: LayoutMode,
        #[case] size: Size,
        #[case] expected: Breakpoint,
    ) {
        assert_eq!(
            Breakpoint::new(size, &Breakpoints::default(), mode),
            expected
        );
    }

    #[test]
    fn a_lowered_minimum_lets_the_minimal_breakpoint_through() {
        let layout = Breakpoints {
            min_columns: Cells(10),
            min_rows: Cells(3),
            ..Breakpoints::default()
        };
        assert_eq!(
            Breakpoint::new(Size::new(20, 5), &layout, LayoutMode::Auto),
            Breakpoint::Minimal
        );
    }
}
