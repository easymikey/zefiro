// GUARD: crates and modules depend downwards only, per `docs/principles.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::guards::support;

const ALL: &[&str] = &[
    "kernel", "audio", "library", "macos", "config", "runtime", "widgets", "terminal",
];

const MODULE_CRATES: &[&str] = &[
    "kernel", "audio", "library", "macos", "config", "runtime", "widgets", "terminal",
    "sifr",
];

const ROOT: &str = "crate";

const KNOWN_CYCLES: &[(&str, &[&str])] = &[];

type Graph = BTreeMap<String, BTreeSet<String>>;

type Paths = Vec<Vec<String>>;

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
fn only_screen_imports_screen() {
    let mut violations = Vec::new();

    for (rel, path) in support::files_in(&["widgets"], "src") {
        if rel.starts_with("widgets/src/screen/") {
            continue;
        }
        let content = support::read(&path);
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("//") && trimmed.contains("crate::screen") {
                violations.push(format!("{rel}:{}: {trimmed}", index + 1));
            }
        }
    }

    support::report(
        "layering guard: no widgets module outside src/screen/ imports \
         crate::screen. There is no allowlist.",
        &violations,
        &[],
    );
}

fn module_name(relative: &str) -> Option<String> {
    let stem = relative.strip_suffix(".rs")?;
    let stem = stem.strip_suffix("/mod").unwrap_or(stem);
    if stem == "lib" || stem == "main" {
        return Some(ROOT.to_owned());
    }
    Some(stem.replace('/', "::"))
}

fn item_end(rest: &str) -> usize {
    let mut depth = 0_usize;
    for (index, ch) in rest.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            ';' if depth == 0 => return index + 1,
            _ => {}
        }
    }
    rest.len()
}

fn production_text(content: &str) -> String {
    let marker = "#[cfg(test)]";
    let mut text: String = content
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    while let Some(start) = text.find(marker) {
        let rest = &text[start + marker.len()..];
        let end = start + marker.len() + item_end(rest);
        text.replace_range(start..end, "");
    }
    text
}

fn tree(input: &str) -> (Paths, &str) {
    let input = input.trim_start();
    if let Some(inner) = input.strip_prefix('{') {
        return group(inner);
    }
    if let Some(after) = input.strip_prefix('*') {
        return (vec![Vec::new()], after);
    }
    let length = input
        .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .unwrap_or(input.len());
    let (segment, after) = input.split_at(length);
    if segment.is_empty() {
        return (Vec::new(), after);
    }
    let Some(next) = after.strip_prefix("::") else {
        return (vec![vec![segment.to_owned()]], after);
    };
    let (tails, remaining) = tree(next);
    if tails.is_empty() {
        return (vec![vec![segment.to_owned()]], remaining);
    }
    let paths = tails
        .into_iter()
        .map(|tail| std::iter::once(segment.to_owned()).chain(tail).collect())
        .collect();
    (paths, remaining)
}

fn group(start: &str) -> (Paths, &str) {
    let mut paths = Vec::new();
    let mut input = start;
    loop {
        input = input.trim_start();
        if let Some(after) = input.strip_prefix('}') {
            return (paths, after);
        }
        if let Some(after) = input.strip_prefix(',') {
            input = after;
            continue;
        }
        let (found, remaining) = tree(input);
        if remaining.len() == input.len() {
            return (paths, remaining);
        }
        paths.extend(found);
        input = remaining;
    }
}

fn crate_paths(text: &str) -> Paths {
    text.match_indices("crate::")
        .filter(|(index, _)| {
            text[..*index]
                .chars()
                .next_back()
                .is_none_or(|ch| !(ch.is_alphanumeric() || "_$:".contains(ch)))
        })
        .flat_map(|(index, marker)| tree(&text[index + marker.len()..]).0)
        .collect()
}

fn resolve(modules: &BTreeSet<String>, path: &[String]) -> String {
    (1..=path.len())
        .rev()
        .map(|length| path[..length].join("::"))
        .find(|candidate| modules.contains(candidate))
        .unwrap_or_else(|| ROOT.to_owned())
}

fn module_graph(crate_name: &str) -> Graph {
    let prefix = format!("{crate_name}/src/");
    let files: Vec<(String, String)> = support::files_in(&[crate_name], "src")
        .into_iter()
        .filter_map(|(relative, path)| {
            let module = module_name(relative.strip_prefix(&prefix)?)?;
            Some((module, support::read(&path)))
        })
        .collect();
    let modules: BTreeSet<String> =
        files.iter().map(|(module, _)| module.clone()).collect();
    files
        .iter()
        .map(|(module, content)| {
            let targets = crate_paths(&production_text(content))
                .iter()
                .map(|path| resolve(&modules, path))
                .filter(|target| target != module)
                .collect();
            (module.clone(), targets)
        })
        .collect()
}

fn reachable(graph: &Graph, start: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.to_owned()];
    while let Some(node) = stack.pop() {
        for next in graph.get(&node).into_iter().flatten() {
            if seen.insert(next.clone()) {
                stack.push(next.clone());
            }
        }
    }
    seen
}

fn cycles(graph: &Graph) -> BTreeSet<Vec<String>> {
    let reach: BTreeMap<&String, BTreeSet<String>> = graph
        .keys()
        .map(|node| (node, reachable(graph, node)))
        .collect();
    reach
        .iter()
        .filter(|(node, targets)| targets.contains(node.as_str()))
        .map(|(node, targets)| {
            targets
                .iter()
                .filter(|other| {
                    reach
                        .get(other)
                        .is_some_and(|back| back.contains(node.as_str()))
                })
                .cloned()
                .collect()
        })
        .collect()
}

fn listed(crate_name: &str, modules: &[String]) -> bool {
    KNOWN_CYCLES.iter().any(|(known_crate, known_modules)| {
        *known_crate == crate_name
            && known_modules
                .iter()
                .copied()
                .eq(modules.iter().map(String::as_str))
    })
}

#[test]
fn module_cycles() {
    let found: Vec<(&str, Vec<String>)> = MODULE_CRATES
        .iter()
        .flat_map(|crate_name| {
            cycles(&module_graph(crate_name))
                .into_iter()
                .map(move |modules| (*crate_name, modules))
        })
        .collect();
    let violations: Vec<String> = found
        .iter()
        .filter(|(crate_name, modules)| !listed(crate_name, modules))
        .map(|(crate_name, modules)| format!("{crate_name}: {}", modules.join(", ")))
        .collect();
    let stale: Vec<String> = KNOWN_CYCLES
        .iter()
        .filter(|(known_crate, known_modules)| {
            !found.iter().any(|(crate_name, modules)| {
                crate_name == known_crate
                    && known_modules
                        .iter()
                        .copied()
                        .eq(modules.iter().map(String::as_str))
            })
        })
        .map(|(known_crate, known_modules)| {
            format!("{known_crate}: {}", known_modules.join(", "))
        })
        .collect();

    support::report(
        "layering guard: a crate's modules import each other over `crate::` paths without \
         a cycle (conventions.md §11.10). `KNOWN_CYCLES` lists today's cycles; each fix \
         deletes its row.",
        &violations,
        &stale,
    );
}

#[test]
fn module_cycles_reads_nested_use_groups_and_skips_test_modules() {
    let text = production_text(
        "use crate::{a::{b, c::D}, e};\nfn f() { crate::g::H::new(); }\n\
         #[cfg(test)]\nmod tests { use crate::z; }\n",
    );
    let paths: Vec<String> = crate_paths(&text)
        .iter()
        .map(|path| path.join("::"))
        .collect();
    assert_eq!(paths, ["a::b", "a::c::D", "e", "g::H::new"]);
}
