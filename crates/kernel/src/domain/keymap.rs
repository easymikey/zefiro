use std::{
    collections::{HashMap, hash_map::Entry},
    fmt,
};

use strum::{EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

use crate::{
    domain::{Chord, ChordParseError},
    update::keymap::{Bindings, KeyBinding},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum KeyContext {
    #[default]
    Global,
    Playlist,
    TextPrompt,
    Search,
    Help,
    History,
    Settings,
    ConfirmDelete,
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
    SettingsAdjustDown,
    SettingsAdjustUp,
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
    pub key_context: KeyContext,
}

impl From<&str> for KeyOverride {
    fn from(chord: &str) -> Self {
        Self {
            chord: chord.to_string(),
            key_context: KeyContext::default(),
        }
    }
}

impl From<String> for KeyOverride {
    fn from(chord: String) -> Self {
        Self {
            chord,
            key_context: KeyContext::default(),
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
pub enum KeyValidationError {
    #[error(transparent)]
    InvalidChord(#[from] ChordParseError),
    #[error("key collision on `{chord}`")]
    ChordCollision { chord: Chord },
    #[error("key collision left `{action}` unbound")]
    ActionUnbound { action: Action },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DefaultBinding {
    action: Action,
    chord: Chord,
    key_context: KeyContext,
}

fn template_bindings(base: &[KeyBinding]) -> Vec<DefaultBinding> {
    base.iter()
        .filter_map(|binding| {
            Some(DefaultBinding {
                action: binding.action?,
                chord: binding.pattern.chord()?,
                key_context: binding.key_context,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Binding {
    chord: Chord,
    key_context: KeyContext,
}

struct Candidate {
    action: Action,
    key_context: KeyContext,
    default_chords: Vec<Chord>,
    configured: Option<Binding>,
}

fn bound(
    rebind: &KeyOverride,
    errors: &mut Vec<KeyValidationError>,
) -> Option<Binding> {
    match rebind.chord.parse::<Chord>() {
        Ok(chord) => Some(Binding {
            chord,
            key_context: rebind.key_context,
        }),
        Err(error) => {
            errors.push(KeyValidationError::InvalidChord(error));
            None
        }
    }
}

fn desired_bindings(
    overrides: &KeymapOverrides,
    defaults: &[DefaultBinding],
) -> (Vec<Candidate>, Vec<KeyValidationError>) {
    let mut errors = Vec::new();
    let desired: Vec<Candidate> = Action::iter()
        .filter_map(|action| {
            let key_context = defaults
                .iter()
                .find(|default| default.action == action)?
                .key_context;
            let default_chords: Vec<Chord> = defaults
                .iter()
                .filter(|default| default.action == action)
                .map(|default| default.chord)
                .collect();
            let configured = overrides
                .get(action)
                .and_then(|rebind| bound(rebind, &mut errors));
            Some(Candidate {
                action,
                key_context,
                default_chords,
                configured,
            })
        })
        .collect();
    (desired, errors)
}

#[derive(Default)]
struct Placement {
    occupied: HashMap<Binding, Action>,
    won: HashMap<Action, Binding>,
}

fn configured_placement(
    desired: &[Candidate],
    errors: &mut Vec<KeyValidationError>,
) -> Placement {
    desired
        .iter()
        .filter_map(|candidate| {
            candidate.configured.map(|slot| (candidate.action, slot))
        })
        .fold(Placement::default(), |mut placement, (action, slot)| {
            match placement.occupied.entry(slot) {
                Entry::Vacant(vacancy) => {
                    vacancy.insert(action);
                    placement.won.insert(action, slot);
                }
                Entry::Occupied(_) => {
                    errors
                        .push(KeyValidationError::ChordCollision { chord: slot.chord });
                }
            }
            placement
        })
}

fn default_chords(
    desired: &[Candidate],
    placement: &mut Placement,
    errors: &mut Vec<KeyValidationError>,
) -> HashMap<Action, Vec<Chord>> {
    desired
        .iter()
        .fold(HashMap::new(), |mut final_chords, candidate| {
            if let Some(&slot) = placement.won.get(&candidate.action) {
                final_chords.insert(candidate.action, vec![slot.chord]);
                return final_chords;
            }
            let placed: Vec<Chord> = candidate
                .default_chords
                .iter()
                .copied()
                .filter(|&chord| {
                    let slot = Binding {
                        chord,
                        key_context: candidate.key_context,
                    };
                    match placement.occupied.entry(slot) {
                        Entry::Vacant(vacancy) => {
                            vacancy.insert(candidate.action);
                            true
                        }
                        Entry::Occupied(_) => false,
                    }
                })
                .collect();
            if placed.is_empty() {
                errors.push(KeyValidationError::ActionUnbound {
                    action: candidate.action,
                });
            } else {
                final_chords.insert(candidate.action, placed);
            }
            final_chords
        })
}

pub(crate) struct Resolution {
    pub(crate) errors: Vec<KeyValidationError>,
    pub(crate) final_chords: HashMap<Action, Vec<Chord>>,
    pub(crate) contexts: HashMap<Action, KeyContext>,
}

pub(crate) fn resolve(overrides: &KeymapOverrides, base: &[KeyBinding]) -> Resolution {
    let (desired, mut errors) = desired_bindings(overrides, &template_bindings(base));
    let mut placement = configured_placement(&desired, &mut errors);
    let final_chords = default_chords(&desired, &mut placement, &mut errors);
    let contexts = placement
        .won
        .into_iter()
        .map(|(action, slot)| (action, slot.key_context))
        .collect();
    Resolution {
        errors,
        final_chords,
        contexts,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Keymap {
    pub(crate) overrides: KeymapOverrides,
    pub(crate) errors: Vec<KeyValidationError>,
    pub(crate) bindings: Bindings,
}

impl Keymap {
    #[must_use]
    pub fn new(overrides: KeymapOverrides) -> Self {
        let (bindings, errors) = Bindings::resolved(&overrides);
        Self {
            overrides,
            errors,
            bindings,
        }
    }

    #[must_use]
    pub fn overrides(&self) -> &KeymapOverrides {
        &self.overrides
    }

    #[must_use]
    pub fn errors(&self) -> &[KeyValidationError] {
        &self.errors
    }

    #[must_use]
    pub fn bindings(&self) -> &[KeyBinding] {
        self.bindings.as_slice()
    }

    pub(crate) fn error_text(&self) -> Option<String> {
        let texts: Vec<String> = self.errors.iter().map(ToString::to_string).collect();
        (!texts.is_empty()).then(|| texts.join("; "))
    }
}

#[cfg(test)]
mod validation_error_tests {
    use rstest::rstest;

    use crate::domain::{
        Action,
        Chord,
        ChordParseError,
        Key,
        KeyCode,
        Modifiers,
        keymap::{KeyValidationError, Keymap},
    };

    #[rstest]
    #[case::an_invalid_chord_names_its_spelling(
        KeyValidationError::InvalidChord(ChordParseError {
            spelling: "not-a-key".into(),
        })
        .to_string(),
        "invalid key chord `not-a-key`"
    )]
    #[case::a_chord_collision_names_the_chord(
        KeyValidationError::ChordCollision {
            chord: Chord::Key(Key {
                code: KeyCode::Char('x'),
                modifiers: Modifiers::NONE,
            }),
        }
        .to_string(),
        "key collision on `x`"
    )]
    #[case::an_unbound_action_names_the_action(
        KeyValidationError::ActionUnbound {
            action: Action::Previous,
        }
        .to_string(),
        "key collision left `Previous` unbound"
    )]
    fn display_renders_the_expected_text(
        #[case] rendered: String,
        #[case] expected: &str,
    ) {
        assert_eq!(rendered, expected);
    }

    #[test]
    fn a_keymap_without_errors_has_no_error_text() {
        assert_eq!(Keymap::default().error_text(), None);
    }

    #[test]
    fn error_text_joins_entries_with_semicolon_space() {
        let keymap = Keymap {
            errors: vec![
                KeyValidationError::InvalidChord(ChordParseError {
                    spelling: "bad".into(),
                }),
                KeyValidationError::ChordCollision {
                    chord: Chord::Key(Key {
                        code: KeyCode::Char('p'),
                        modifiers: Modifiers::NONE,
                    }),
                },
            ],
            ..Keymap::default()
        };
        assert_eq!(
            keymap.error_text().as_deref(),
            Some("invalid key chord `bad`; key collision on `p`")
        );
    }
}
