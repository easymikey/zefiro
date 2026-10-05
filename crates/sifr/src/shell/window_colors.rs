use std::convert::Infallible;

use kernel::{
    cmd::{Cmd, WindowColorsCmd},
    domain::theme::ThemeName,
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WindowColorsWrite {
    Done,
    Staged(ThemeName),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WindowColorsMessage {
    Commanded(WindowColorsCmd),
    Painted {
        theme_name: ThemeName,
        wash: Option<f32>,
    },
}

impl Machine for WindowColorsWrite {
    type Message = WindowColorsMessage;
    type Effect = Cmd<WindowColorsCmd, Infallible>;

    fn transition(
        &mut self,
        message: WindowColorsMessage,
    ) -> Result<Self::Effect, Unhandled> {
        match (&*self, message) {
            (_, WindowColorsMessage::Commanded(WindowColorsCmd::Set(name))) => {
                *self = WindowColorsWrite::Staged(name);
                Ok(Cmd::none())
            }
            (_, WindowColorsMessage::Commanded(WindowColorsCmd::Reset)) => {
                *self = WindowColorsWrite::Done;
                Ok(Cmd::effect(WindowColorsCmd::Reset))
            }
            (
                WindowColorsWrite::Staged(staged),
                WindowColorsMessage::Painted { theme_name, wash },
            ) if *staged == theme_name && wash.is_none() => {
                *self = WindowColorsWrite::Done;
                Ok(Cmd::effect(WindowColorsCmd::Set(theme_name)))
            }
            (
                WindowColorsWrite::Staged(_) | WindowColorsWrite::Done,
                WindowColorsMessage::Painted { .. },
            ) => Err(Unhandled),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use kernel::{
        cmd::{Cmd, WindowColorsCmd},
        domain::theme::ThemeName,
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::shell::window_colors::{WindowColorsMessage, WindowColorsWrite};

    struct Row {
        window_colors_write: WindowColorsWrite,
        message: WindowColorsMessage,
        next: WindowColorsWrite,
        result: Result<Cmd<WindowColorsCmd, Infallible>, Unhandled>,
    }

    fn name(text: &'static str) -> ThemeName {
        ThemeName::from_static(text)
    }

    fn staged(text: &'static str) -> WindowColorsWrite {
        WindowColorsWrite::Staged(name(text))
    }

    fn painted(text: &'static str, wash: Option<f32>) -> WindowColorsMessage {
        WindowColorsMessage::Painted {
            theme_name: name(text),
            wash,
        }
    }

    #[rstest]
    #[case::set_waits_for_its_own_paint(Row {
        window_colors_write: WindowColorsWrite::Done,
        message: WindowColorsMessage::Commanded(WindowColorsCmd::Set(name("ghost"))),
        next: staged("ghost"),
        result: Ok(Cmd::none()),
    })]
    #[case::reset_writes_at_once(Row {
        window_colors_write: staged("ghost"),
        message: WindowColorsMessage::Commanded(WindowColorsCmd::Reset),
        next: WindowColorsWrite::Done,
        result: Ok(Cmd::effect(WindowColorsCmd::Reset)),
    })]
    #[case::own_theme_painted_writes(Row {
        window_colors_write: staged("ghost"),
        message: painted("ghost", None),
        next: WindowColorsWrite::Done,
        result: Ok(Cmd::effect(WindowColorsCmd::Set(name("ghost")))),
    })]
    #[case::running_wash_defers_the_write(Row {
        window_colors_write: staged("ghost"),
        message: painted("ghost", Some(0.5)),
        next: staged("ghost"),
        result: Err(Unhandled),
    })]
    #[case::stale_theme_is_dropped(Row {
        window_colors_write: staged("ghost"),
        message: painted("other", None),
        next: staged("ghost"),
        result: Err(Unhandled),
    })]
    #[case::nothing_staged_drops_a_paint(Row {
        window_colors_write: WindowColorsWrite::Done,
        message: painted("ghost", None),
        next: WindowColorsWrite::Done,
        result: Err(Unhandled),
    })]
    fn window_colors_follow_the_transition_table(#[case] row: Row) {
        let mut machine = row.window_colors_write;

        let result = machine.transition(row.message);

        assert_eq!(result, row.result);
        assert_eq!(machine, row.next);
    }
}
