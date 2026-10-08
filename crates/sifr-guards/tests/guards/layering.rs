// GUARD: crates and modules depend downwards only, per `docs/principles.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::guards::support;

const ALL: &[&str] = &[
    "kernel", "audio", "library", "macos", "config", "remote", "runtime", "widgets",
    "terminal",
];

const ROOT: &str = "crate";

type Graph = BTreeMap<String, BTreeSet<String>>;

type Paths = Vec<Vec<String>>;

fn allowed_deps(crate_name: &str) -> Option<&'static [&'static str]> {
    match crate_name {
        "kernel" => Some(&[]),
        "audio" | "library" | "macos" | "config" | "remote" => Some(&["kernel"]),
        "runtime" => Some(&["audio", "library", "macos", "config", "remote", "kernel"]),
        "widgets" => Some(&["kernel"]),
        "terminal" => Some(&["kernel", "widgets"]),
        "sifr" => Some(ALL),
        "sifr-guards" => Some(&[]),
        _ => None,
    }
}

fn layer_violations(
    members: &[String],
    manifests: &[(String, toml::Value)],
) -> Vec<String> {
    members
        .iter()
        .flat_map(|member| {
            let Some(allowed) = allowed_deps(member) else {
                return vec![format!(
                    "workspace member `{member}` has no row in `allowed_deps` — add one \
                     to this guard"
                )];
            };
            let Some((_, doc)) = manifests.iter().find(|(crate_name, _)| crate_name == member)
            else {
                return vec![format!("workspace member `{member}` has no manifest under crates/")];
            };
            let mut deps = BTreeSet::new();
            support::dependency_names(doc, support::RUNTIME_TABLES, &mut deps);
            deps.into_iter()
                .filter(|dep_name| {
                    members.contains(dep_name) && !allowed.contains(&dep_name.as_str())
                })
                .map(|dep_name| format!("{member} -> {dep_name} is not in the allow-map"))
                .collect()
        })
        .collect()
}

#[test]
fn crate_dependencies_only_point_left() {
    let members = support::workspace_members();
    assert!(
        !members.is_empty(),
        "expected to read the workspace members"
    );

    support::report(
        "layering guard: every workspace member has an allow-map entry and depends only on \
         the crates it names (docs/principles.md Level 4).",
        &layer_violations(&members, &support::manifests()),
    );
}

#[test]
fn layer_violations_names_a_member_missing_from_the_allow_map() {
    let manifest = |text: &str| toml::from_str::<toml::Value>(text).unwrap();
    let members = ["kernel", "audio", "jukebox"].map(str::to_owned);
    let manifests = [
        ("kernel".to_owned(), manifest("[dependencies]\n")),
        (
            "audio".to_owned(),
            manifest("[dependencies]\nkernel = {}\n"),
        ),
        (
            "jukebox".to_owned(),
            manifest("[dependencies]\naudio = {}\n"),
        ),
    ];
    assert_eq!(
        layer_violations(&members, &manifests),
        [
            "workspace member `jukebox` has no row in `allowed_deps` — add one to this \
             guard"
        ]
    );
}

const DEPENDENCY_HOMES: &[(&str, &[&str])] = &[
    ("ureq", &["remote"]),
    ("md5", &["remote"]),
    ("keyring-core", &["remote", "macos"]),
    ("apple-native-keyring-store", &["macos"]),
    ("objc2", &["macos"]),
    ("block2", &["macos"]),
    ("dispatch2", &["macos"]),
    ("cpal", &["audio"]),
    ("symphonia", &["audio"]),
];

fn misplaced_dependencies(crate_name: &str, manifest: &toml::Value) -> Vec<String> {
    let mut deps = BTreeSet::new();
    support::dependency_names(manifest, support::DEPENDENCY_TABLES, &mut deps);
    deps.iter()
        .filter_map(|dep_name| {
            let (family, homes) = DEPENDENCY_HOMES.iter().find(|(family, _)| {
                dep_name
                    .strip_prefix(family)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
            })?;
            (!homes.contains(&crate_name)).then(|| {
                format!(
                    "{crate_name} -> {dep_name}: the `{family}` crates belong only in {}",
                    homes.join(", ")
                )
            })
        })
        .collect()
}

#[test]
fn network_keychain_os_and_audio_crates_stay_in_their_crate() {
    let violations: Vec<String> = support::manifests()
        .iter()
        .flat_map(|(crate_name, doc)| misplaced_dependencies(crate_name, doc))
        .collect();

    support::report(
        "layering guard: ureq and md5 live only in remote, keyring-core in remote and macos, \
         the keychain store and the objc2 crates in macos, cpal and symphonia in audio \
         (streaming stage: audio never has network).",
        &violations,
    );
}

#[test]
fn misplaced_dependencies_names_the_crate_and_the_dependency() {
    let manifest = toml::from_str::<toml::Value>(
        "[dependencies]\ncpal = {}\nureq = {}\n\
         [target.'cfg(target_os = \"macos\")'.dev-dependencies]\nobjc2-foundation = {}\n",
    )
    .unwrap();
    assert_eq!(
        misplaced_dependencies("audio", &manifest),
        [
            "audio -> objc2-foundation: the `objc2` crates belong only in macos",
            "audio -> ureq: the `ureq` crates belong only in remote",
        ]
    );
}

#[test]
fn misplaced_dependencies_reads_the_package_of_a_renamed_dependency() {
    let manifest = toml::from_str::<toml::Value>(
        "[dependencies]\nhttp = { package = \"ureq\" }\n",
    )
    .unwrap();
    assert_eq!(
        misplaced_dependencies("audio", &manifest),
        ["audio -> ureq: the `ureq` crates belong only in remote"]
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

#[test]
fn module_cycles() {
    let dirs = support::member_dirs(&support::read(
        &support::workspace_root().join("Cargo.toml"),
    ));
    let violations: Vec<String> = dirs
        .iter()
        .flat_map(|crate_name| {
            cycles(&module_graph(crate_name))
                .into_iter()
                .map(move |modules| format!("{crate_name}: {}", modules.join(", ")))
        })
        .collect();

    support::report(
        "layering guard: a crate's modules import each other over `crate::` paths without \
         a cycle (conventions.md §11.10).",
        &violations,
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
