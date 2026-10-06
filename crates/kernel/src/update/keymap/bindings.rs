use std::{collections::HashMap, fmt};

use strum::IntoEnumIterator;

use crate::{
    domain::{
        chord::{Chord, KeyPattern},
        config::Diagnostic,
        keymap::{Action, KeyContext, KeyOverride, KeymapError, KeymapOverrides},
    },
    update::keymap::{
        chord::{BindingOrigin, KeyBinding},
        table,
    },
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

struct Candidate {
    action: Action,
    key_context: KeyContext,
    default_chords: Vec<Chord>,
    binding: Option<Binding>,
}

fn parsed(key_override: &KeyOverride) -> Result<Binding, KeymapError> {
    let chord = key_override.chord.parse::<Chord>()?;
    Ok(Binding {
        chord,
        key_context: key_override.key_context,
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
                let (configured, error) =
                    match keymap_overrides.get(action).map(parsed).transpose() {
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
    action: Action,
    binding: Binding,
}

fn placed(
    entries: impl IntoIterator<Item = ChordAssignment>,
) -> (Vec<ChordAssignment>, Vec<ChordAssignment>) {
    let entries = entries.into_iter().collect::<Vec<_>>();
    entries.iter().copied().partition(|entry| {
        entries
            .iter()
            .find(|earlier| earlier.binding == entry.binding)
            == Some(entry)
    })
}

fn configured_placement(candidates: &[Candidate]) -> Vec<ChordAssignment> {
    candidates
        .iter()
        .filter_map(|candidate| {
            Some(ChordAssignment {
                action: candidate.action,
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
                    action: candidate.action,
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
    let (configured, refused) = placed(configured_placement(&candidates));
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
                .filter(|placement| placement.action == candidate.action)
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
        .map(|placement| (placement.action, placement.binding.key_context))
        .collect();
    Resolution {
        errors,
        final_chords,
        contexts,
    }
}

#[derive(Debug, Clone)]
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

    let (configured, fallback): (Vec<KeyBinding>, Vec<KeyBinding>) = Action::iter()
        .filter_map(|action| {
            let template =
                base.iter().find(|binding| binding.action == Some(action))?;
            let chords = final_chords.get(&action)?;
            let configured = contexts.get(&action).copied();
            let key_context = configured.unwrap_or(template.key_context);
            Some(chords.iter().map(move |&chord| KeyBinding {
                pattern: KeyPattern::Chord(chord),
                origin: if configured.is_some() {
                    BindingOrigin::Configured
                } else {
                    BindingOrigin::Default
                },
                key_context,
                ..template.clone()
            }))
        })
        .flatten()
        .partition(|binding| matches!(binding.origin, BindingOrigin::Configured));
    let key_bindings = configured
        .into_iter()
        .chain(fallback)
        .chain(
            base.iter()
                .filter(|binding| {
                    !binding.action.is_some_and(|action| {
                        Action::iter().any(|rebindable| rebindable == action)
                    })
                })
                .cloned(),
        )
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
}
