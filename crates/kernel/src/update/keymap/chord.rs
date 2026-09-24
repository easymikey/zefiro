use crate::{
    domain::{Action, CharSink, Chord, KeyContext, KeyPattern},
    message::Message,
};

#[derive(Debug, Clone)]
pub enum KeyOutcome {
    Message(Message),
    TypeChar(CharSink),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingSource {
    Configured,
    Default,
}

#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub pattern: KeyPattern,
    pub outcome: KeyOutcome,
    pub action: Option<Action>,
    pub key_context: KeyContext,
    pub(super) source: BindingSource,
}

pub(super) struct ActionRow {
    pub(super) action: Action,
    pub(super) chord: Chord,
    pub(super) message: Message,
    pub(super) key_context: KeyContext,
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

pub(super) struct KeyContextRow {
    pub(super) key_context: KeyContext,
    pub(super) pattern: KeyPattern,
    pub(super) outcome: KeyOutcome,
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
