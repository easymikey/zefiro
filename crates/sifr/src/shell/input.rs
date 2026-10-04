use crossterm::event::{Event, KeyEvent, KeyEventKind};
use kernel::{Key, KeyPress, Message, PaintError, PaintEvent};
use runtime::Reaction;
use terminal::{LayoutTranslation, from_event};

#[derive(Debug, Clone)]
pub(crate) enum ShellInput {
    Terminal(Event),
    Terminate,
    Error(PaintError),
}

pub(crate) fn reaction_for(input: ShellInput) -> Reaction {
    match input {
        ShellInput::Terminate => Reaction::Message(Message::Quit),
        ShellInput::Error(error) => {
            Reaction::Message(Message::from(PaintEvent::Error(error)))
        }
        ShellInput::Terminal(event) => terminal_reaction(&event),
    }
}

fn terminal_reaction(event: &Event) -> Reaction {
    match event {
        Event::Key(key_event) => key_reaction(*key_event),
        Event::Resize(_, _) | Event::FocusGained => Reaction::Repaint,
        Event::FocusLost | Event::Mouse(_) | Event::Paste(_) => Reaction::Ignored,
    }
}

fn key_reaction(key_event: KeyEvent) -> Reaction {
    if key_event.kind != KeyEventKind::Press {
        return Reaction::Ignored;
    }
    let key = from_event(key_event, LayoutTranslation::Applied);
    let typed = from_event(key_event, LayoutTranslation::Verbatim);
    key_press(key, typed).map_or(Reaction::Ignored, |press| {
        Reaction::Message(Message::Key(press))
    })
}

fn key_press(key: Option<Key>, typed: Option<Key>) -> Option<KeyPress> {
    let key = key.or(typed)?;
    Some(KeyPress {
        key,
        typed: typed.unwrap_or(key),
    })
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
    use kernel::{Diagnostic, Key, KeyCode, KeyPress, Message, PaintError, PaintEvent};
    use rstest::rstest;
    use runtime::Reaction;

    use crate::shell::input::{ShellInput, reaction_for};

    fn key_input(character: char) -> ShellInput {
        ShellInput::Terminal(Event::Key(KeyEvent::new(
            CrosstermCode::Char(character),
            KeyModifiers::NONE,
        )))
    }

    #[test]
    fn a_key_press_becomes_a_key_message() {
        let reaction = reaction_for(key_input(' '));

        let key = Key::plain(KeyCode::Char(' '));
        assert_eq!(
            reaction,
            Reaction::Message(Message::Key(KeyPress { key, typed: key }))
        );
    }

    #[test]
    fn terminate_quits_without_touching_the_model() {
        let reaction = reaction_for(ShellInput::Terminate);

        assert_eq!(reaction, Reaction::Message(Message::Quit));
    }

    #[test]
    fn a_probe_failure_becomes_a_paint_error_message() {
        let error = PaintError::Probe(Diagnostic::from_error(&std::io::Error::other(
            "no answer",
        )));

        let reaction = reaction_for(ShellInput::Error(error.clone()));

        assert_eq!(
            reaction,
            Reaction::Message(Message::from(PaintEvent::Error(error)))
        );
    }

    #[rstest]
    #[case::a_resize(ShellInput::Terminal(Event::Resize(80, 24)))]
    #[case::focus_gained(ShellInput::Terminal(Event::FocusGained))]
    fn resize_and_focus_gained_ask_for_a_repaint(#[case] input: ShellInput) {
        let reaction = reaction_for(input);

        assert_eq!(reaction, Reaction::Repaint);
    }
}
