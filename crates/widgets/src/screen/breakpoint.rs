use config::{BreakpointsConfig, LayoutMode};
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
    pub fn new(size: Size, breakpoints: &BreakpointsConfig) -> Self {
        if fit(size, breakpoints.min_columns, breakpoints.min_rows) == Fit::Short {
            return Self::TooSmall;
        }
        let full = fit(
            size,
            breakpoints.full_min_width,
            breakpoints.full_min_height,
        );
        let compact = fit(
            size,
            breakpoints.compact_min_width,
            breakpoints.compact_min_height,
        );
        match (breakpoints.mode, full, compact) {
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
    use config::{BreakpointsConfig, LayoutMode};
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
        let breakpoints = BreakpointsConfig {
            mode,
            ..BreakpointsConfig::default()
        };
        assert_eq!(Breakpoint::new(size, &breakpoints), expected);
    }

    #[test]
    fn a_lowered_minimum_lets_the_minimal_breakpoint_through() {
        let breakpoints = BreakpointsConfig {
            min_columns: 10,
            min_rows: 3,
            ..BreakpointsConfig::default()
        };
        assert_eq!(
            Breakpoint::new(Size::new(20, 5), &breakpoints),
            Breakpoint::Minimal
        );
    }
}
