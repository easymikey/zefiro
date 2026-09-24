use crossterm::event::{Event, KeyEvent, KeyEventKind};
use kernel::{
    Message,
    Overlay,
    domain::{Model, TextCapture},
    update::keymap::{Bindings, KeyPress, route},
};
use runtime::Reaction;
use terminal::{LayoutTranslation, from_event};

use crate::toast::{ShellFailure, toast_message};

#[derive(Debug, Clone)]
pub(crate) enum ShellInput {
    Terminal(Event),
    Terminate,
    Failed(ShellFailure),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RouteContext<'a> {
    pub(crate) model: &'a Model,
    pub(crate) bindings: &'a Bindings,
    pub(crate) page_size: usize,
}

pub(crate) fn message_for(input: ShellInput, context: RouteContext<'_>) -> Reaction {
    match input {
        ShellInput::Terminate => Reaction::Message(Message::Quit),
        ShellInput::Terminal(event) => terminal_message(&event, context),
        ShellInput::Failed(failure) => Reaction::Message(toast_message(&failure)),
    }
}

fn terminal_message(event: &Event, context: RouteContext<'_>) -> Reaction {
    match event {
        Event::Key(key_event) => keyboard_message(*key_event, context),
        Event::Resize(_, _) | Event::FocusGained => Reaction::Repaint,
        Event::FocusLost | Event::Mouse(_) | Event::Paste(_) => Reaction::Ignored,
    }
}

fn keyboard_message(key_event: KeyEvent, context: RouteContext<'_>) -> Reaction {
    if key_event.kind != KeyEventKind::Press {
        return Reaction::Ignored;
    }
    let Some(key) = from_event(key_event, layout_translation(context.model)) else {
        return Reaction::Ignored;
    };
    route(
        context.bindings,
        &context.model.workspace,
        KeyPress {
            key,
            visible_rows: context.page_size,
        },
    )
    .map_or(Reaction::Ignored, Reaction::Message)
}

fn layout_translation(model: &Model) -> LayoutTranslation {
    match model.workspace.overlay.as_ref().map(Overlay::captures_text) {
        Some(TextCapture::Typing) => LayoutTranslation::Verbatim,
        Some(TextCapture::Chording) | None => LayoutTranslation::Applied,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode as CrosstermCode, KeyEvent, KeyModifiers};
    use kernel::{
        BrowseRequest,
        Message,
        PlaybackRequest,
        domain::{KeymapOverrides, Model},
        update::keymap::Bindings,
    };
    use rstest::rstest;
    use runtime::Reaction;

    use crate::{
        shell::input::{RouteContext, ShellInput, message_for},
        toast::{ShellFailure, toast_message},
    };

    fn key_press(character: char) -> ShellInput {
        ShellInput::Terminal(Event::Key(KeyEvent::new(
            CrosstermCode::Char(character),
            KeyModifiers::NONE,
        )))
    }

    fn context<'a>(model: &'a Model, bindings: &'a Bindings) -> RouteContext<'a> {
        RouteContext {
            model,
            bindings,
            page_size: 0,
        }
    }

    #[test]
    fn a_space_press_toggles_playback() {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let message = message_for(key_press(' '), context(&model, &bindings));

        assert_eq!(
            message,
            Reaction::Message(Message::Playback(PlaybackRequest::Toggle))
        );
    }

    #[test]
    fn a_j_press_moves_the_cursor_down() {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let message = message_for(key_press('j'), context(&model, &bindings));

        assert_eq!(
            message,
            Reaction::Message(Message::Browse(BrowseRequest::CursorBy(1)))
        );
    }

    #[test]
    fn a_q_press_quits() {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let message = message_for(key_press('q'), context(&model, &bindings));

        assert_eq!(message, Reaction::Message(Message::Quit));
    }

    #[test]
    fn terminate_quits_without_touching_the_model() {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let message = message_for(ShellInput::Terminate, context(&model, &bindings));

        assert_eq!(message, Reaction::Message(Message::Quit));
    }

    #[test]
    fn a_failure_becomes_a_toast_message_at_once() {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let failure = ShellFailure::Cover("broken".to_string());
        let message = message_for(
            ShellInput::Failed(failure.clone()),
            context(&model, &bindings),
        );

        assert_eq!(message, Reaction::Message(toast_message(&failure)));
    }

    #[rstest]
    #[case::a_resize(ShellInput::Terminal(Event::Resize(80, 24)))]
    #[case::focus_gained(ShellInput::Terminal(Event::FocusGained))]
    fn resize_and_focus_gained_ask_for_a_repaint(#[case] input: ShellInput) {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let message = message_for(input, context(&model, &bindings));

        assert_eq!(message, Reaction::Repaint);
    }

    #[test]
    fn an_unbound_letter_is_ignored() {
        let model = Model::default();
        let bindings = Bindings::new(&KeymapOverrides::default());
        let message = message_for(key_press('w'), context(&model, &bindings));

        assert_eq!(message, Reaction::Ignored);
    }
}
