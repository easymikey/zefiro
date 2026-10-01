use crate::{
    domain::{
        Action,
        CharSink,
        Chord,
        Key,
        KeyCode,
        KeyContext,
        KeyPattern,
        Modifiers,
    },
    message::Message,
};

#[derive(Debug, Clone)]
pub enum KeyOutcome {
    Message(Message),
    TypeChar(CharSink),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingSource {
    Configured,
    Default,
}

#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub pattern: KeyPattern,
    pub outcome: KeyOutcome,
    pub action: Option<Action>,
    pub key_context: KeyContext,
    pub(crate) source: BindingSource,
}

pub(crate) struct ActionRow {
    pub(crate) action: Action,
    pub(crate) chord: Chord,
    pub(crate) message: Message,
    pub(crate) key_context: KeyContext,
}

impl From<ActionRow> for KeyBinding {
    fn from(row: ActionRow) -> Self {
        Self {
            pattern: KeyPattern::Chord(row.chord),
            outcome: KeyOutcome::Message(row.message),
            action: Some(row.action),
            key_context: row.key_context,
            source: BindingSource::Default,
        }
    }
}

pub(crate) struct KeyContextRow {
    pub(crate) key_context: KeyContext,
    pub(crate) pattern: KeyPattern,
    pub(crate) outcome: KeyOutcome,
}

impl From<KeyContextRow> for KeyBinding {
    fn from(row: KeyContextRow) -> Self {
        Self {
            pattern: row.pattern,
            outcome: row.outcome,
            action: None,
            key_context: row.key_context,
            source: BindingSource::Default,
        }
    }
}

pub(crate) fn key(character: char) -> Chord {
    bare(KeyCode::Char(character))
}

pub(crate) fn bare(code: KeyCode) -> Chord {
    Chord::Key(Key::plain(code))
}

pub(crate) fn shifted(code: KeyCode) -> Chord {
    Chord::Key(Key::new(code, Modifiers::SHIFT))
}

pub(crate) fn ctrl(character: char) -> Chord {
    Chord::Key(Key::ctrl(KeyCode::Char(character)))
}
