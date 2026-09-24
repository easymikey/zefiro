use std::collections::HashSet;

use strum::IntoEnumIterator;

use crate::{
    domain::{
        Action,
        DefaultBinding,
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

#[must_use]
pub fn default_bindings() -> Vec<DefaultBinding> {
    template_bindings(table::defaults())
}

fn resolved_bindings(
    config: &KeymapOverrides,
) -> (Vec<KeyBinding>, Vec<KeyValidationError>) {
    let base = table::defaults();
    let defaults = template_bindings(base);
    let Resolution {
        errors,
        final_chords,
        contexts,
    } = resolve(config, &defaults);

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
            Some(chords.iter().map(move |&chord| {
                let mut binding = template.clone();
                binding.pattern = KeyPattern::Chord(chord);
                binding.source = if configured.is_some() {
                    BindingSource::Configured
                } else {
                    BindingSource::Default
                };
                binding.key_context = key_context;
                binding
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
    pub fn new(config: &KeymapOverrides) -> Self {
        let (bindings, _) = resolved_bindings(config);
        Self(bindings)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[KeyBinding] {
        &self.0
    }
}
