use crossterm::event::{Event, KeyEvent, KeyEventKind};
use kernel::{Key, KeyPress, Message};
use runtime::Reaction;
use terminal::{LayoutTranslation, from_event};

#[derive(Debug, Clone)]
pub(crate) enum ShellInput {
    Terminal(Event),
    Terminate,
}

pub(crate) fn message_for(input: ShellInput) -> Reaction {
    match input {
        ShellInput::Terminate => Reaction::Message(Message::Quit),
        ShellInput::Terminal(event) => terminal_message(&event),
    }
}

fn terminal_message(event: &Event) -> Reaction {
    match event {
        Event::Key(key_event) => keyboard_message(*key_event),
        Event::Resize(_, _) | Event::FocusGained => Reaction::Repaint,
        Event::FocusLost | Event::Mouse(_) | Event::Paste(_) => Reaction::Ignored,
    }
}

fn keyboard_message(key_event: KeyEvent) -> Reaction {
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
    match (key, typed) {
        (Some(key), Some(typed)) => Some(KeyPress { key, typed }),
        (Some(key), None) => Some(KeyPress { key, typed: key }),
        (None, Some(typed)) => Some(KeyPress { key: typed, typed }),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
    use kernel::{Key, KeyCode, KeyPress, Message};
    use rstest::rstest;
    use runtime::Reaction;

    use crate::shell::input::{ShellInput, message_for};

    fn key_press(character: char) -> ShellInput {
        ShellInput::Terminal(Event::Key(KeyEvent::new(
            CrosstermCode::Char(character),
            KeyModifiers::NONE,
        )))
    }

    #[test]
    fn a_key_press_becomes_a_key_message() {
        let message = message_for(key_press(' '));

        let key = Key::plain(KeyCode::Char(' '));
        assert_eq!(
            message,
            Reaction::Message(Message::Key(KeyPress { key, typed: key }))
        );
    }

    #[test]
    fn terminate_quits_without_touching_the_model() {
        let message = message_for(ShellInput::Terminate);

        assert_eq!(message, Reaction::Message(Message::Quit));
    }

    #[rstest]
    #[case::a_resize(ShellInput::Terminal(Event::Resize(80, 24)))]
    #[case::focus_gained(ShellInput::Terminal(Event::FocusGained))]
    fn resize_and_focus_gained_ask_for_a_repaint(#[case] input: ShellInput) {
        let message = message_for(input);

        assert_eq!(message, Reaction::Repaint);
    }
}
