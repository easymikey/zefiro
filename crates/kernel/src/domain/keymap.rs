use std::{collections::HashMap, fmt};

use strum::{EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

use crate::domain::chord::{Chord, ChordError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum KeyContext {
    Global,
    Playlist,
    TextPrompt,
    Search,
    Help,
    History,
    Settings,
    #[strum(serialize = "confirm_delete")]
    ConfirmTrash,
    JumpToTime,
    TrackDetails,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, EnumString, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum Action {
    PlayPause,
    Next,
    Previous,
    SeekBack,
    SeekForward,
    SeekBackShort,
    SeekForwardShort,
    SeekBackLong,
    SeekForwardLong,
    VolumeUp,
    VolumeDown,
    Shuffle,
    Repeat,
    SleepTimer,
    AbRepeat,
    SpeedDown,
    SpeedUp,
    JumpToTime,
    Down,
    Up,
    Top,
    Bottom,
    PageDown,
    PageUp,
    PlaySelected,
    Enqueue,
    PlayNext,
    Dequeue,
    QueueMoveUp,
    QueueMoveDown,
    CycleSort,
    Favorite,
    Delete,
    SavePlaylist,
    FullScan,
    TrackDetails,
    Search,
    History,
    Settings,
    SettingsNavigateDown,
    SettingsNavigateUp,
    SettingsStepDown,
    SettingsStepUp,
    SettingsActivate,
    SettingsClose,
    MusicDir,
    Help,
    Quit,
    #[strum(disabled)]
    SeekTenth(u8),
}

impl fmt::Display for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOverride {
    pub chord: String,
    pub key_context: Option<KeyContext>,
}

impl From<&str> for KeyOverride {
    fn from(chord: &str) -> Self {
        Self {
            chord: chord.to_string(),
            key_context: None,
        }
    }
}

impl From<String> for KeyOverride {
    fn from(chord: String) -> Self {
        Self {
            chord,
            key_context: None,
        }
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct KeymapOverrides(HashMap<Action, KeyOverride>);

impl KeymapOverrides {
    #[must_use]
    pub fn get(&self, action: Action) -> Option<&KeyOverride> {
        self.0.get(&action)
    }
}

impl<const COUNT: usize> From<[(Action, KeyOverride); COUNT]> for KeymapOverrides {
    fn from(entries: [(Action, KeyOverride); COUNT]) -> Self {
        Self(HashMap::from(entries))
    }
}

impl FromIterator<(Action, KeyOverride)> for KeymapOverrides {
    fn from_iter<I: IntoIterator<Item = (Action, KeyOverride)>>(entries: I) -> Self {
        Self(entries.into_iter().collect())
    }
}

impl fmt::Debug for KeymapOverrides {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_map()
            .entries(Action::iter().filter_map(|action| {
                let spelling: &'static str = action.into();
                Some((spelling, self.0.get(&action)?))
            }))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error(transparent)]
    InvalidChord(#[from] ChordError),
    #[error("key collision on `{0}`")]
    ChordCollision(Chord),
    #[error("key collision left `{0}` unbound")]
    ActionUnbound(Action),
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{
        chord::{Chord, ChordError},
        key::{Key, KeyCode, Modifiers},
        keymap::{Action, KeymapError},
    };

    #[rstest]
    #[case::an_invalid_chord_names_its_spelling(
        KeymapError::InvalidChord(ChordError {
            spelling: "not-a-key".into(),
        })
        .to_string(),
        "invalid key chord `not-a-key`"
    )]
    #[case::a_chord_collision_names_the_chord(
        KeymapError::ChordCollision(Chord::Key(Key {
            code: KeyCode::Char('x'),
            modifiers: Modifiers::NONE,
        }))
        .to_string(),
        "key collision on `x`"
    )]
    #[case::an_unbound_action_names_the_action(
        KeymapError::ActionUnbound(Action::Previous)
        .to_string(),
        "key collision left `Previous` unbound"
    )]
    fn display_renders_the_expected_text(
        #[case] rendered: String,
        #[case] expected: &str,
    ) {
        assert_eq!(rendered, expected);
    }
}
