use config::Animations;
use kernel::WindowColorsCmd;
use terminal::{UnknownThemeError, WindowColorsWriter};
use widgets::Theme;

pub(crate) fn window_colors(
    theme: &Theme,
    command: WindowColorsCmd,
) -> Option<UnknownThemeError> {
    let (sender, receiver) = crossbeam_channel::unbounded();
    let _ = sender.send(command);
    WindowColorsWriter::new(receiver)
        .obey(|name| (name == theme.name).then(|| theme.clone()))
}

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

pub(crate) fn settle_window_colors_plan(
    pending: PendingWindowColors,
    wash_progress: Option<f32>,
) -> bool {
    if wash_progress.is_some() {
        return false;
    }
    matches!(pending, PendingWindowColors::Staged)
}

#[cfg(test)]
mod tests {
    use config::Animations;

    use crate::shell::window_colors::{
        PendingWindowColors,
        WindowColorsPlan,
        settle_window_colors_plan,
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
        assert!(!settle_window_colors_plan(
            PendingWindowColors::Staged,
            Some(0.4)
        ));
    }

    #[test]
    fn a_finished_wash_releases_the_staged_plan() {
        assert!(settle_window_colors_plan(PendingWindowColors::Staged, None));
    }

    #[test]
    fn no_wash_and_nothing_staged_settles_to_nothing() {
        assert!(!settle_window_colors_plan(PendingWindowColors::Idle, None));
    }
}
