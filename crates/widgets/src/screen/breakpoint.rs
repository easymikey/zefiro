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
    Enough,
    Short,
}

fn fit(size: Size, width: u16, height: u16) -> Fit {
    if size.width >= width && size.height >= height {
        Fit::Enough
    } else {
        Fit::Short
    }
}

impl Breakpoint {
    #[must_use]
    pub fn new(size: Size, breakpoints: &Breakpoints, layout_mode: LayoutMode) -> Self {
        if fit(size, breakpoints.min_width.0, breakpoints.min_height.0) == Fit::Short {
            return Self::TooSmall;
        }
        let full = fit(
            size,
            breakpoints.full_min_width.0,
            breakpoints.full_min_height.0,
        );
        let compact = fit(
            size,
            breakpoints.compact_min_width.0,
            breakpoints.compact_min_height.0,
        );
        match (layout_mode, full, compact) {
            (LayoutMode::Compact, _, Fit::Enough)
            | (LayoutMode::Auto, Fit::Short, Fit::Enough) => Self::Compact,
            (LayoutMode::Auto | LayoutMode::Compact, Fit::Enough, _) => Self::Full,
            (LayoutMode::Auto | LayoutMode::Compact, Fit::Short, Fit::Short) => {
                Self::Minimal
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::appearance::{Breakpoints, LayoutMode};
    use ratatui::layout::Size;
    use rstest::rstest;

    use crate::screen::breakpoint::Breakpoint;

    #[rstest]
    #[case::full_edge(LayoutMode::Auto, Size::new(60, 19), Breakpoint::Full)]
    #[case::narrow(LayoutMode::Auto, Size::new(59, 24), Breakpoint::Compact)]
    #[case::compact_asked(LayoutMode::Compact, Size::new(80, 24), Breakpoint::Compact)]
    fn the_terminal_size_picks_the_breakpoint(
        #[case] layout_mode: LayoutMode,
        #[case] size: Size,
        #[case] expected: Breakpoint,
    ) {
        assert_eq!(
            Breakpoint::new(size, &Breakpoints::default(), layout_mode),
            expected
        );
    }
}
