use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt,
};

use strum::IntoEnumIterator;

use crate::{
    domain::{
        chord::{Chord, KeyPattern},
        config::Diagnostic,
        keymap::{
            Action,
            KeyContext,
            KeyOverride,
            KeyValidationError,
            KeymapOverrides,
        },
    },
    update::keymap::{
        chord::{BindingSource, KeyBinding},
        table,
    },
};

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
                    errors.push(KeyValidationError::ChordCollision(slot.chord));
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
                errors.push(KeyValidationError::ActionUnbound(candidate.action));
            } else {
                final_chords.insert(candidate.action, placed);
            }
            final_chords
        })
}

struct Resolution {
    errors: Vec<KeyValidationError>,
    final_chords: HashMap<Action, Vec<Chord>>,
    contexts: HashMap<Action, KeyContext>,
}

fn resolve(overrides: &KeymapOverrides, base: &[KeyBinding]) -> Resolution {
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

#[derive(Debug, Clone)]
pub struct Keymap {
    pub(crate) overrides: KeymapOverrides,
    pub(crate) errors: Vec<KeyValidationError>,
    bindings: Vec<KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(KeymapOverrides::default())
    }
}

impl Keymap {
    #[must_use]
    pub fn new(overrides: KeymapOverrides) -> Self {
        let (bindings, errors) = resolved_bindings(&overrides);
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
    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    pub(crate) fn diagnostic(&self) -> Option<Diagnostic> {
        (!self.errors.is_empty())
            .then(|| Diagnostic::from_error(&KeyValidationErrors(&self.errors)))
    }
}

#[derive(Debug)]
struct KeyValidationErrors<'a>(&'a [KeyValidationError]);

impl fmt::Display for KeyValidationErrors<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let texts: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        formatter.write_str(&texts.join("; "))
    }
}

impl std::error::Error for KeyValidationErrors<'_> {}

fn resolved_bindings(
    overrides: &KeymapOverrides,
) -> (Vec<KeyBinding>, Vec<KeyValidationError>) {
    let base = table::defaults();
    let Resolution {
        errors,
        final_chords,
        contexts,
    } = resolve(overrides, &base);

    let rebindable: HashSet<Action> = Action::iter()
        .filter(|&action| base.iter().any(|binding| binding.action == Some(action)))
        .collect();
    let mut out: Vec<KeyBinding> = Action::iter()
        .filter_map(|action| {
            let template =
                base.iter().find(|binding| binding.action == Some(action))?;
            let chords = final_chords.get(&action)?;
            let configured = contexts.get(&action).copied();
            let key_context = configured.unwrap_or(template.key_context);
            Some(chords.iter().map(move |&chord| KeyBinding {
                pattern: KeyPattern::Chord(chord),
                source: if configured.is_some() {
                    BindingSource::Configured
                } else {
                    BindingSource::Default
                },
                key_context,
                ..template.clone()
            }))
        })
        .flatten()
        .collect();
    out.sort_by_key(|binding| !matches!(binding.source, BindingSource::Configured));
    out.extend(
        base.iter()
            .filter(|&binding| {
                !binding
                    .action
                    .is_some_and(|action| rebindable.contains(&action))
            })
            .cloned(),
    );
    (out, errors)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            chord::{Chord, ChordParseError},
            config::Diagnostic,
            key::{Key, KeyCode, Modifiers},
            keymap::KeyValidationError,
        },
        update::keymap::bindings::Keymap,
    };

    #[test]
    fn the_shipped_keymap_has_no_chord_collisions() {
        assert_eq!(Keymap::default().diagnostic(), None);
    }

    #[test]
    fn error_text_joins_entries_with_semicolon_space() {
        let keymap = Keymap {
            errors: vec![
                KeyValidationError::InvalidChord(ChordParseError {
                    spelling: "bad".into(),
                }),
                KeyValidationError::ChordCollision(Chord::Key(Key {
                    code: KeyCode::Char('p'),
                    modifiers: Modifiers::NONE,
                })),
            ],
            ..Keymap::default()
        };
        assert_eq!(
            keymap.diagnostic().as_ref().map(Diagnostic::text),
            Some("invalid key chord `bad`; key collision on `p`")
        );
    }
}
