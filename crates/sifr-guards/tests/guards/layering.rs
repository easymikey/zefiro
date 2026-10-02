// GUARD: crates and modules depend downwards only, per `docs/principles.md`.

use std::collections::BTreeSet;

use crate::guards::support;

const ALL: &[&str] = &[
    "kernel", "audio", "library", "macos", "config", "runtime", "widgets", "terminal",
];

fn allowed_deps(crate_name: &str) -> Option<&'static [&'static str]> {
    match crate_name {
        "kernel" => Some(&[]),
        "audio" | "library" | "macos" | "config" => Some(&["kernel"]),
        "runtime" => Some(&["audio", "library", "macos", "config", "kernel"]),
        "widgets" => Some(&["kernel"]),
        "terminal" => Some(&["kernel", "widgets"]),
        "sifr" => Some(ALL),
        _ => None,
    }
}

#[test]
fn crate_dependencies_only_point_left() {
    let found = support::manifests();
    assert!(!found.is_empty(), "expected to find the crate manifests");

    let mut violations = Vec::new();
    let mut unplaced = Vec::new();

    for (crate_name, doc) in found {
        let Some(allowed) = allowed_deps(&crate_name) else {
            continue;
        };

        let mut deps = BTreeSet::new();
        support::sifr_runtime_dependencies(&doc, &mut deps);

        for dep_name in deps {
            if allowed_deps(&dep_name).is_none() {
                unplaced.push(format!(
                    "unknown sifr crate `{dep_name}` referenced by `{crate_name}` — add it \
                     to `allowed_deps` in this guard"
                ));
            } else if !allowed.contains(&dep_name.as_str()) {
                violations.push(format!(
                    "{crate_name} -> {dep_name} is not in the allow-map"
                ));
            }
        }
    }

    violations.extend(unplaced);

    support::report(
        "layering guard: a crate depends only on the crates its allow-map entry names \
         (docs/principles.md Level 4).",
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
