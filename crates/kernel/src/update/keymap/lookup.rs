use crate::{
    domain::{
        CharSink,
        Chord,
        ChordPrefix,
        Key,
        KeyCode,
        KeyContext,
        KeyPattern,
        KeyPress,
        Overlay,
        TextCapture,
        Workspace,
    },
    message::{
        BrowseRequest,
        Message,
        OverlayRequest,
        SearchEdit,
        SearchRequest,
        TextRequest,
    },
    update::keymap::{
        chord::{KeyBinding, KeyOutcome},
        key_context::key_context_stack,
    },
};

#[must_use]
pub fn route(workspace: &Workspace, press: KeyPress) -> Option<Message> {
    let key = pressed_key(workspace, press);
    let bindings = workspace.bindings.as_slice();
    let stack = key_context_stack(workspace);
    let lookup = |key_context| {
        in_key_context(
            bindings,
            &BindingScope {
                key_context,
                prefix: workspace.chord,
            },
            key,
        )
    };
    lookup(stack.primary()).or_else(|| stack.fallback().and_then(lookup))
}

fn pressed_key(workspace: &Workspace, press: KeyPress) -> Key {
    match workspace.overlay.as_ref().map(Overlay::captures_text) {
        Some(TextCapture::Typing) => press.typed,
        Some(TextCapture::Chording) | None => press.key,
    }
}

struct BindingScope {
    key_context: KeyContext,
    prefix: Option<ChordPrefix>,
}

fn in_key_context(
    bindings: &[KeyBinding],
    lookup: &BindingScope,
    key: Key,
) -> Option<Message> {
    if let Some(prefix) = lookup.prefix
        && let Some(message) = looked_up(
            bindings,
            lookup.key_context,
            KeyPattern::Chord(Chord::Sequence { prefix, key }),
        )
    {
        return Some(message);
    }
    if let Some(prefix) = armable_prefix(bindings, lookup.key_context, key) {
        return Some(Message::Browse(BrowseRequest::ChordPrefix(prefix)));
    }
    bindings
        .iter()
        .filter(|binding| binding.key_context == lookup.key_context)
        .find(|binding| matched(binding.pattern, key))
        .and_then(|binding| message_of(&binding.outcome, key))
}

fn looked_up(
    bindings: &[KeyBinding],
    key_context: KeyContext,
    pattern: KeyPattern,
) -> Option<Message> {
    bindings
        .iter()
        .filter(|binding| binding.key_context == key_context)
        .find(|binding| binding.pattern == pattern)
        .and_then(|binding| match &binding.outcome {
            KeyOutcome::Message(message) => Some(message.clone()),
            KeyOutcome::TypeChar(_) => None,
        })
}

fn armable_prefix(
    bindings: &[KeyBinding],
    key_context: KeyContext,
    key: Key,
) -> Option<ChordPrefix> {
    let prefix = ChordPrefix::from_key(key)?;
    bindings
        .iter()
        .filter(|binding| binding.key_context == key_context)
        .any(|binding| starts_with(binding.pattern, prefix))
        .then_some(prefix)
}

fn starts_with(pattern: KeyPattern, prefix: ChordPrefix) -> bool {
    match pattern {
        KeyPattern::Chord(Chord::Sequence { prefix: armed, .. }) => armed == prefix,
        KeyPattern::Chord(Chord::Key(_)) | KeyPattern::AnyChar | KeyPattern::AnyKey => {
            false
        }
    }
}

fn matched(pattern: KeyPattern, key: Key) -> bool {
    match pattern {
        KeyPattern::Chord(Chord::Key(bound)) => bound == key,
        KeyPattern::Chord(Chord::Sequence { .. }) => false,
        KeyPattern::AnyChar => matches!(key.code, KeyCode::Char(_)),
        KeyPattern::AnyKey => true,
    }
}

fn message_of(outcome: &KeyOutcome, key: Key) -> Option<Message> {
    match outcome {
        KeyOutcome::Message(message) => Some(message.clone()),
        KeyOutcome::TypeChar(sink) => match key.code {
            KeyCode::Char(character) => Some(typed(*sink, character)),
            KeyCode::Enter
            | KeyCode::Esc
            | KeyCode::Backspace
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Tab
            | KeyCode::PageUp
            | KeyCode::PageDown => None,
        },
    }
}

fn typed(sink: CharSink, character: char) -> Message {
    Message::Overlay(match sink {
        CharSink::Text => OverlayRequest::Text(TextRequest::Char(character)),
        CharSink::Search => {
            OverlayRequest::Search(SearchRequest::Edit(SearchEdit::Char(character)))
        }
    })
}
