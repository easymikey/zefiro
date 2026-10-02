use config::LayoutMode;
use kernel::domain::appearance::Breakpoints;
use ratatui::layout::Size;
use widgets::Breakpoint;

#[test]
fn a_zero_size_terminal_is_too_small() {
    assert_eq!(
        Breakpoint::new(Size::new(0, 0), &Breakpoints::default(), LayoutMode::Auto),
        Breakpoint::TooSmall
    );
}

#[test]
fn a_very_large_terminal_is_full() {
    assert_eq!(
        Breakpoint::new(
            Size::new(300, 100),
            &Breakpoints::default(),
            LayoutMode::Auto
        ),
        Breakpoint::Full
    );
}

#[test]
fn custom_breakpoints_are_honored_not_just_defaults() {
    let breakpoints = Breakpoints {
        full_min_width: 10,
        full_min_height: 10,
        compact_min_width: 5,
        compact_min_height: 5,
        min_columns: 5,
        min_rows: 5,
    };
    assert_eq!(
        Breakpoint::new(Size::new(10, 10), &breakpoints, LayoutMode::Auto),
        Breakpoint::Full
    );
    assert_eq!(
        Breakpoint::new(Size::new(5, 5), &breakpoints, LayoutMode::Auto),
        Breakpoint::Compact
    );
    assert_eq!(
        Breakpoint::new(Size::new(4, 5), &breakpoints, LayoutMode::Auto),
        Breakpoint::TooSmall
    );
}

#[test]
fn a_compact_override_at_its_own_floor_stays_compact() {
    let breakpoints = Breakpoints {
        min_columns: 20,
        min_rows: 3,
        ..Breakpoints::default()
    };
    assert_eq!(
        Breakpoint::new(
            Size::new(
                breakpoints.compact_min_width,
                breakpoints.compact_min_height
            ),
            &breakpoints,
            LayoutMode::Compact
        ),
        Breakpoint::Compact
    );
}

#[test]
fn a_compact_override_below_its_own_floor_falls_back_to_minimal() {
    let breakpoints = Breakpoints {
        min_columns: 20,
        min_rows: 3,
        ..Breakpoints::default()
    };
    assert_eq!(
        Breakpoint::new(
            Size::new(
                breakpoints.compact_min_width - 1,
                breakpoints.compact_min_height
            ),
            &breakpoints,
            LayoutMode::Compact
        ),
        Breakpoint::Minimal
    );
}
