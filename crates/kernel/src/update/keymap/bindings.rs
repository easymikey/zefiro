use std::{collections::HashMap, fmt};

use strum::IntoEnumIterator;

use crate::{
    domain::{
        chord::{Chord, KeyPattern},
        config::Diagnostic,
        keymap::{Action, KeyContext, KeyOverride, KeymapError, KeymapOverrides},
    },
    update::keymap::{chord::KeyBinding, table},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DefaultBinding {
    action: Action,
    chord: Chord,
    key_context: KeyContext,
}

fn default_bindings(default_key_bindings: &[KeyBinding]) -> Vec<DefaultBinding> {
    default_key_bindings
        .iter()
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

fn lane(key_context: KeyContext) -> KeyContext {
    match key_context {
        KeyContext::Global | KeyContext::Playlist => KeyContext::Global,
        overlay @ (KeyContext::TextPrompt
        | KeyContext::Search
        | KeyContext::Help
        | KeyContext::History
        | KeyContext::Settings
        | KeyContext::ConfirmTrash
        | KeyContext::JumpToTime
        | KeyContext::TrackDetails) => overlay,
    }
}

impl Binding {
    fn collides(&self, other: &Self) -> bool {
        lane(self.key_context) == lane(other.key_context)
            && match (self.chord, other.chord) {
                (
                    Chord::Key(key),
                    Chord::Sequence {
                        prefix: chord_prefix,
                        key: _key,
                    },
                )
                | (
                    Chord::Sequence {
                        prefix: chord_prefix,
                        key: _key,
                    },
                    Chord::Key(key),
                ) => key == chord_prefix.key(),
                (Chord::Key(_), Chord::Key(_))
                | (Chord::Sequence { .. }, Chord::Sequence { .. }) => {
                    self.chord == other.chord
                }
            }
    }
}

struct Candidate {
    action: Action,
    key_context: KeyContext,
    default_chords: Vec<Chord>,
    binding: Option<Binding>,
}

fn parsed(
    key_override: &KeyOverride,
    default_context: KeyContext,
) -> Result<Binding, KeymapError> {
    let chord = key_override.chord.parse::<Chord>()?;
    Ok(Binding {
        chord,
        key_context: key_override.key_context.unwrap_or(default_context),
    })
}

fn candidates(
    keymap_overrides: &KeymapOverrides,
    default_bindings: &[DefaultBinding],
) -> (Vec<Candidate>, Vec<KeymapError>) {
    let (candidates, errors): (Vec<Candidate>, Vec<Option<KeymapError>>) =
        Action::iter()
            .filter_map(|action| {
                let key_context = default_bindings
                    .iter()
                    .find(|default| default.action == action)?
                    .key_context;
                let default_chords: Vec<Chord> = default_bindings
                    .iter()
                    .filter(|default| default.action == action)
                    .map(|default| default.chord)
                    .collect();
                let (configured, error) = match keymap_overrides
                    .get(action)
                    .map(|key_override| parsed(key_override, key_context))
                    .transpose()
                {
                    Ok(configured) => (configured, None),
                    Err(error) => (None, Some(error)),
                };
                Some((
                    Candidate {
                        action,
                        key_context,
                        default_chords,
                        binding: configured,
                    },
                    error,
                ))
            })
            .unzip();
    (candidates, errors.into_iter().flatten().collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChordAssignment {
    action: Option<Action>,
    binding: Binding,
}

fn fixed(key_binding: &KeyBinding) -> bool {
    matches!(key_binding.action, None | Some(Action::SeekTenth(_)))
}

fn fixed_chord_assignments(
    default_key_bindings: &[KeyBinding],
) -> Vec<ChordAssignment> {
    default_key_bindings
        .iter()
        .filter(|binding| fixed(binding))
        .filter_map(|binding| {
            Some(ChordAssignment {
                action: binding.action,
                binding: Binding {
                    chord: binding.pattern.chord()?,
                    key_context: binding.key_context,
                },
            })
        })
        .collect()
}

fn placed(
    entries: impl IntoIterator<Item = ChordAssignment>,
) -> (Vec<ChordAssignment>, Vec<ChordAssignment>) {
    let entries = entries.into_iter().collect::<Vec<_>>();
    entries.iter().copied().partition(|entry| {
        entries
            .iter()
            .find(|earlier| earlier.binding.collides(&entry.binding))
            == Some(entry)
    })
}

fn configured_placement(candidates: &[Candidate]) -> Vec<ChordAssignment> {
    candidates
        .iter()
        .filter_map(|candidate| {
            Some(ChordAssignment {
                action: Some(candidate.action),
                binding: candidate.binding?,
            })
        })
        .collect()
}

fn final_chords(candidates: &[Candidate]) -> Vec<ChordAssignment> {
    candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .default_chords
                .iter()
                .map(move |&chord| ChordAssignment {
                    action: Some(candidate.action),
                    binding: Binding {
                        chord,
                        key_context: candidate.key_context,
                    },
                })
        })
        .collect()
}

struct Resolution {
    errors: Vec<KeymapError>,
    final_chords: HashMap<Action, Vec<Chord>>,
    contexts: HashMap<Action, KeyContext>,
}

fn resolution(
    keymap_overrides: &KeymapOverrides,
    default_key_bindings: &[KeyBinding],
) -> Resolution {
    let (candidates, errors) =
        candidates(keymap_overrides, &default_bindings(default_key_bindings));
    let (configured, refused) = placed(
        fixed_chord_assignments(default_key_bindings)
            .into_iter()
            .chain(configured_placement(&candidates)),
    );
    let (settled, _) = placed(configured.iter().copied().chain(
        final_chords(&candidates).into_iter().filter(|entry| {
            configured
                .iter()
                .all(|placement| placement.action != entry.action)
        }),
    ));
    let final_chords: HashMap<Action, Vec<Chord>> = candidates
        .iter()
        .filter_map(|candidate| {
            let chords: Vec<Chord> = settled
                .iter()
                .filter(|placement| placement.action == Some(candidate.action))
                .map(|placement| placement.binding.chord)
                .collect();
            (!chords.is_empty()).then_some((candidate.action, chords))
        })
        .collect();
    let errors = errors
        .into_iter()
        .chain(
            refused
                .iter()
                .map(|placement| KeymapError::ChordCollision(placement.binding.chord)),
        )
        .chain(
            candidates
                .iter()
                .filter(|candidate| !final_chords.contains_key(&candidate.action))
                .map(|candidate| KeymapError::ActionUnbound(candidate.action)),
        )
        .collect();
    let contexts = configured
        .iter()
        .filter_map(|placement| {
            Some((placement.action?, placement.binding.key_context))
        })
        .collect();
    Resolution {
        errors,
        final_chords,
        contexts,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    pub(crate) keymap_overrides: KeymapOverrides,
    errors: Vec<KeymapError>,
    key_bindings: Vec<KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(KeymapOverrides::default())
    }
}

impl Keymap {
    #[must_use]
    pub fn new(keymap_overrides: KeymapOverrides) -> Self {
        let (bindings, errors) = resolved_bindings(&keymap_overrides);
        Self {
            keymap_overrides,
            errors,
            key_bindings: bindings,
        }
    }

    #[must_use]
    pub fn overrides(&self) -> &KeymapOverrides {
        &self.keymap_overrides
    }

    #[must_use]
    pub fn bindings(&self) -> &[KeyBinding] {
        &self.key_bindings
    }

    pub(crate) fn diagnostic(&self) -> Option<Diagnostic> {
        (!self.errors.is_empty())
            .then(|| Diagnostic::from_error(&KeymapErrors(&self.errors)))
    }
}

#[derive(Debug)]
struct KeymapErrors<'a>(&'a [KeymapError]);

impl fmt::Display for KeymapErrors<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let texts: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        formatter.write_str(&texts.join("; "))
    }
}

impl std::error::Error for KeymapErrors<'_> {}

fn resolved_bindings(
    keymap_overrides: &KeymapOverrides,
) -> (Vec<KeyBinding>, Vec<KeymapError>) {
    let base = table::defaults();
    let Resolution {
        errors,
        final_chords,
        contexts,
    } = resolution(keymap_overrides, &base);

    let key_bindings = Action::iter()
        .filter_map(|action| {
            let template =
                base.iter().find(|binding| binding.action == Some(action))?;
            let chords = final_chords.get(&action)?;
            let key_context = contexts
                .get(&action)
                .copied()
                .unwrap_or(template.key_context);
            Some(chords.iter().map(move |&chord| KeyBinding {
                pattern: KeyPattern::Chord(chord),
                key_context,
                ..template.clone()
            }))
        })
        .flatten()
        .chain(base.iter().filter(|binding| fixed(binding)).cloned())
        .collect();
    (key_bindings, errors)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            config::Diagnostic,
            keymap::{Action, KeyOverride, KeymapOverrides},
        },
        update::keymap::bindings::Keymap,
    };

    #[test]
    fn the_shipped_keymap_has_no_chord_collisions() {
        assert_eq!(Keymap::default().diagnostic(), None);
    }

    #[test]
    fn error_text_joins_entries_with_semicolon_space() {
        let keymap_overrides = KeymapOverrides::from([
            (Action::PlayPause, KeyOverride::from("bad")),
            (Action::Next, KeyOverride::from("y")),
            (Action::Previous, KeyOverride::from("y")),
        ]);
        assert_eq!(
            Keymap::new(keymap_overrides)
                .diagnostic()
                .as_ref()
                .map(Diagnostic::text),
            Some("invalid key chord `bad`; key collision on `y`")
        );
    }

    #[test]
    fn a_configured_chord_on_a_fixed_chord_is_a_collision() {
        let keymap_overrides =
            KeymapOverrides::from([(Action::Next, KeyOverride::from("5"))]);
        assert_eq!(
            Keymap::new(keymap_overrides)
                .diagnostic()
                .as_ref()
                .map(Diagnostic::text),
            Some("key collision on `5`")
        );
    }
}
