// GUARD: crates and modules depend downwards only, in the order
// `docs/principles.md` sets out.

use std::collections::BTreeSet;

use crate::guards::support;

fn layer_of(crate_name: &str) -> Option<u8> {
    match crate_name {
        "kernel" => Some(1),
        "library" | "audio" | "macos" | "config" => Some(2),
        "runtime" => Some(3),
        "widgets" => Some(4),
        "terminal" => Some(5),
        "sifr" => Some(6),
        _ => None,
    }
}

fn allowed_dep_layers(crate_layer: u8) -> &'static [u8] {
    match crate_layer {
        1 => &[],
        2 => &[1],
        3 => &[1, 2],
        4 => &[1, 2, 3],
        5 => &[1, 2, 3, 4],
        6 => &[1, 2, 3, 4, 5],
        _ => &[],
    }
}

#[test]
fn crate_dependencies_only_point_left() {
    let found = support::manifests();
    assert!(!found.is_empty(), "expected to find the crate manifests");

    let mut violations = Vec::new();
    let mut unplaced = Vec::new();

    for (crate_name, doc) in found {
        let Some(crate_layer) = layer_of(&crate_name) else {
            continue;
        };
        let allowed = allowed_dep_layers(crate_layer);

        let mut deps = BTreeSet::new();
        support::sifr_dependencies(&doc, &mut deps);

        for dep_name in deps {
            let Some(dep_layer) = layer_of(&dep_name) else {
                unplaced.push(format!(
                    "unknown sifr crate `{dep_name}` referenced by `{crate_name}` — add it \
                     to `layer_of` in this guard"
                ));
                continue;
            };
            if !allowed.contains(&dep_layer) {
                violations.push(format!(
                    "{crate_name} -> {dep_name} (layer {crate_layer} -> {dep_layer})"
                ));
            }
        }
    }

    violations.extend(unplaced);

    support::report(
        "layering guard: a crate depends only on crates strictly to its left \
         (docs/principles.md Level 4). There is no allowlist.",
        &violations,
        &[],
    );
}

#[test]
fn components_never_import_screen() {
    let mut violations = Vec::new();

    for (rel, path) in support::files_in(&["widgets"], "src/components") {
        let content = support::read(&path);
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("//") && trimmed.contains("crate::screen") {
                violations.push(format!("{rel}:{}: {trimmed}", index + 1));
            }
        }
    }

    support::report(
        "layering guard: components sit below screen — nothing under \
         widgets/src/components may import crate::screen. There is no allowlist.",
        &violations,
        &[],
    );
}
