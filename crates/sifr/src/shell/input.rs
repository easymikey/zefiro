use std::{
    io,
    panic::{self, AssertUnwindSafe},
    thread,
};

use crossbeam_channel::Sender;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use kernel::message::Message;
use runtime::shell::Reaction;
use terminal::keys::key_press;

use crate::{
    shell::shell_input::ShellInput,
    termination::{remember_input_error, remember_worker_panic},
};

pub(crate) fn reaction_for(input: ShellInput) -> Reaction {
    match input {
        ShellInput::Terminate => Reaction::Message(Message::Quit),
        ShellInput::Error(error) => Reaction::Message(Message::from(error)),
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
    key_press(key_event).map_or(Reaction::Ignored, |press| {
        Reaction::Message(Message::Key(press))
    })
}

pub(crate) fn spawn_input(sender: Sender<ShellInput>) {
    drop(thread::spawn(move || {
        match panic::catch_unwind(AssertUnwindSafe(|| read_input(&sender))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => remember_input_error(error),
            Err(_panic) => remember_worker_panic(),
        }
    }));
}

fn read_input(sender: &Sender<ShellInput>) -> Result<(), io::Error> {
    loop {
        let event = event::read()?;
        if sender.send(ShellInput::Terminal(event)).is_err() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
    use kernel::{
        domain::{
            config::Diagnostic,
            key::{Key, KeyCode, KeyPress},
        },
        message::{Message, PaintError},
    };
    use rstest::rstest;
    use runtime::shell::Reaction;

    use crate::shell::{input::reaction_for, shell_input::ShellInput};

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
        let error = PaintError::Query(Diagnostic::from_error(&std::io::Error::other(
            "no answer",
        )));

        let reaction = reaction_for(ShellInput::Error(error.clone()));

        assert_eq!(reaction, Reaction::Message(Message::from(error)));
    }

    #[rstest]
    #[case::a_resize(ShellInput::Terminal(Event::Resize(80, 24)))]
    #[case::focus_gained(ShellInput::Terminal(Event::FocusGained))]
    fn resize_and_focus_gained_ask_for_a_repaint(#[case] input: ShellInput) {
        let reaction = reaction_for(input);

        assert_eq!(reaction, Reaction::Repaint);
    }
}
