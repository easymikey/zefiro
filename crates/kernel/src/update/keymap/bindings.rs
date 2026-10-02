use std::collections::HashSet;

use strum::IntoEnumIterator;

use crate::{
    domain::{
        Action,
        KeyPattern,
        KeyValidationError,
        KeymapOverrides,
        keymap::{Resolution, resolve},
    },
    update::keymap::{
        chord::{BindingSource, KeyBinding},
        table,
    },
};

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

#[derive(Debug, Clone)]
pub struct Bindings(Vec<KeyBinding>);

impl Bindings {
    #[must_use]
    pub fn new(overrides: &KeymapOverrides) -> Self {
        Self::resolved(overrides).0
    }

    pub(crate) fn resolved(
        overrides: &KeymapOverrides,
    ) -> (Self, Vec<KeyValidationError>) {
        let (bindings, errors) = resolved_bindings(overrides);
        (Self(bindings), errors)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[KeyBinding] {
        &self.0
    }
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings::new(&KeymapOverrides::default())
    }
}
