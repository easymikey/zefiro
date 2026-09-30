use config::Animations;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingWindowColors {
    Idle,
    Staged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowColorsPlan {
    ApplyNow,
    Defer,
}

pub(crate) fn window_colors_plan(animations: Animations) -> WindowColorsPlan {
    match animations {
        Animations::Off => WindowColorsPlan::ApplyNow,
        Animations::On => WindowColorsPlan::Defer,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wash {
    Running,
    Idle,
}

pub(crate) fn flush_staged_window_colors_plan(
    pending: PendingWindowColors,
    wash: Wash,
) -> bool {
    match wash {
        Wash::Running => false,
        Wash::Idle => matches!(pending, PendingWindowColors::Staged),
    }
}

#[cfg(test)]
mod tests {
    use config::Animations;

    use crate::shell::window_colors::{
        PendingWindowColors,
        Wash,
        WindowColorsPlan,
        flush_staged_window_colors_plan,
        window_colors_plan,
    };

    #[test]
    fn animations_off_applies_at_once() {
        assert_eq!(
            window_colors_plan(Animations::Off),
            WindowColorsPlan::ApplyNow
        );
    }

    #[test]
    fn animations_on_defers() {
        assert_eq!(window_colors_plan(Animations::On), WindowColorsPlan::Defer);
    }

    #[test]
    fn a_running_wash_holds_a_staged_plan_back() {
        assert!(!flush_staged_window_colors_plan(
            PendingWindowColors::Staged,
            Wash::Running
        ));
    }

    #[test]
    fn a_finished_wash_releases_the_staged_plan() {
        assert!(flush_staged_window_colors_plan(
            PendingWindowColors::Staged,
            Wash::Idle
        ));
    }

    #[test]
    fn no_wash_and_nothing_staged_settles_to_nothing() {
        assert!(!flush_staged_window_colors_plan(
            PendingWindowColors::Idle,
            Wash::Idle
        ));
    }
}
