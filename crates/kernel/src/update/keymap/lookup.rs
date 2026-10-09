use crate::{
    domain::{
        chord::{Chord, ChordPrefix, KeyPattern},
        key::{Key, KeyCode, KeyPress},
        keymap::KeyContext,
        overlay::Overlay,
        workspace::Workspace,
    },
    message::{Message, OverlayRequest, SearchRequest, TextRequest},
    update::keymap::{chord::KeyBinding, key_context::key_context_of},
};

#[must_use]
pub fn route(workspace: &Workspace, press: KeyPress) -> Option<Message> {
    let key = pressed_key(workspace, press);
    let bindings = workspace.keymap.bindings();
    let lookup = |key_context| {
        in_key_context(
            bindings,
            &BindingScope {
                key_context,
                prefix: workspace.chord_prefix,
            },
            key,
        )
    };
    workspace.overlay.as_ref().map(key_context_of).map_or_else(
        || lookup(KeyContext::Playlist).or_else(|| lookup(KeyContext::Global)),
        |context| lookup(context).or_else(|| typed_input(context, key)),
    )
}

fn pressed_key(workspace: &Workspace, press: KeyPress) -> Key {
    if workspace
        .overlay
        .as_ref()
        .is_some_and(Overlay::captures_text)
    {
        press.typed
    } else {
        press.key
    }
}

struct BindingScope {
    key_context: KeyContext,
    prefix: Option<ChordPrefix>,
}

fn in_key_context(
    bindings: &[KeyBinding],
    binding_scope: &BindingScope,
    key: Key,
) -> Option<Message> {
    if let Some(chord_prefix) = binding_scope.prefix
        && let Some(message) = exact_match(
            bindings,
            binding_scope.key_context,
            KeyPattern::Chord(Chord::Sequence { chord_prefix, key }),
        )
    {
        return Some(message);
    }
    if let Some(chord_prefix) = armable_prefix(bindings, binding_scope.key_context, key)
    {
        return Some(Message::ChordPrefix(chord_prefix));
    }
    bindings
        .iter()
        .filter(|binding| binding.key_context == binding_scope.key_context)
        .find(|binding| is_match(binding.pattern, key))
        .map(|binding| binding.message.clone())
}

fn exact_match(
    bindings: &[KeyBinding],
    key_context: KeyContext,
    pattern: KeyPattern,
) -> Option<Message> {
    bindings
        .iter()
        .filter(|binding| binding.key_context == key_context)
        .find(|binding| binding.pattern == pattern)
        .map(|binding| binding.message.clone())
}

fn armable_prefix(
    bindings: &[KeyBinding],
    key_context: KeyContext,
    key: Key,
) -> Option<ChordPrefix> {
    let chord_prefix = ChordPrefix::from_key(key)?;
    bindings
        .iter()
        .filter(|binding| binding.key_context == key_context)
        .any(|binding| starts_with(binding.pattern, chord_prefix))
        .then_some(chord_prefix)
}

fn starts_with(pattern: KeyPattern, chord_prefix: ChordPrefix) -> bool {
    match pattern {
        KeyPattern::Chord(Chord::Sequence {
            chord_prefix: armed,
            ..
        }) => armed == chord_prefix,
        KeyPattern::Chord(Chord::Key(_)) | KeyPattern::AnyKey => false,
    }
}

fn is_match(pattern: KeyPattern, key: Key) -> bool {
    match pattern {
        KeyPattern::Chord(Chord::Key(bound)) => bound == key,
        KeyPattern::Chord(Chord::Sequence { .. }) => false,
        KeyPattern::AnyKey => true,
    }
}

fn typed_input(key_context: KeyContext, key: Key) -> Option<Message> {
    let KeyCode::Char(character) = key.code else {
        return None;
    };
    match key_context {
        KeyContext::TextPrompt | KeyContext::JumpToTime => Some(Message::Overlay(
            OverlayRequest::Text(TextRequest::Char(character)),
        )),
        KeyContext::Search => Some(Message::Overlay(OverlayRequest::Search(
            SearchRequest::Edit(TextRequest::Char(character)),
        ))),
        KeyContext::Global
        | KeyContext::Playlist
        | KeyContext::Help
        | KeyContext::History
        | KeyContext::Settings
        | KeyContext::ConfirmTrash
        | KeyContext::TrackDetails
        | KeyContext::Servers
        | KeyContext::ConfirmRemove => None,
    }
}
