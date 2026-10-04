use crate::{
    domain::{
        Chord,
        ChordPrefix,
        Key,
        KeyCode,
        KeyContext,
        KeyPattern,
        KeyPress,
        Overlay,
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
    update::keymap::{chord::KeyBinding, key_context::key_context_stack},
};

#[must_use]
pub fn route(workspace: &Workspace, press: KeyPress) -> Option<Message> {
    let key = pressed_key(workspace, press);
    let bindings = workspace.keymap.bindings();
    let stack = key_context_stack(workspace);
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
    lookup(stack.primary())
        .or_else(|| typed_input(stack.primary(), key))
        .or_else(|| stack.fallback().and_then(lookup))
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
        .map(|binding| binding.message.clone())
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
        .map(|binding| binding.message.clone())
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
        KeyPattern::Chord(Chord::Key(_)) | KeyPattern::AnyKey => false,
    }
}

fn matched(pattern: KeyPattern, key: Key) -> bool {
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
        KeyContext::TextPrompt => Some(Message::Overlay(OverlayRequest::Text(
            TextRequest::Char(character),
        ))),
        KeyContext::Search => Some(Message::Overlay(OverlayRequest::Search(
            SearchRequest::Edit(SearchEdit::Char(character)),
        ))),
        KeyContext::Global
        | KeyContext::Playlist
        | KeyContext::Help
        | KeyContext::History
        | KeyContext::Settings
        | KeyContext::ConfirmDelete
        | KeyContext::JumpToTime
        | KeyContext::TrackDetails => None,
    }
}
