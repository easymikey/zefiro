// GUARD: the one directory walk, allowlist row and stale-row check every guard
// here is built on.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct Rule {
    pub(crate) pattern: &'static str,
    pub(crate) message: &'static str,
}

pub(crate) fn rule_hit<'a>(line: &str, rules: &'a [Rule]) -> Option<&'a Rule> {
    rules.iter().find(|rule| line.contains(rule.pattern))
}

pub(crate) struct Allow {
    pub(crate) path: &'static str,
    pub(crate) pattern: &'static str,
    pub(crate) reason: &'static str,
}

impl Allow {
    pub(crate) const fn new(
        path: &'static str,
        pattern: &'static str,
        reason: &'static str,
    ) -> Self {
        Self {
            path,
            pattern,
            reason,
        }
    }
}

pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub(crate) fn crates_dir() -> PathBuf {
    workspace_root().join("crates")
}

pub(crate) fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, root, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            out.push((relative.to_string_lossy().replace('\\', "/"), path));
        }
    }
}

pub(crate) fn source_files(areas: &[&str]) -> Vec<(String, PathBuf)> {
    let root = crates_dir();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        for area in areas {
            walk(&crate_dir.join(area), &root, &mut out);
        }
    }
    out.sort();
    out
}

pub(crate) fn files_in(crate_names: &[&str], area: &str) -> Vec<(String, PathBuf)> {
    let root = crates_dir();
    let mut out = Vec::new();
    for crate_name in crate_names {
        walk(&root.join(crate_name).join(area), &root, &mut out);
    }
    out.sort();
    out
}

pub(crate) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

pub(crate) fn manifests() -> Vec<(String, toml::Value)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(crates_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(text) = fs::read_to_string(entry.path().join("Cargo.toml")) else {
            continue;
        };
        let Ok(doc) = toml::from_str::<toml::Value>(&text) else {
            continue;
        };
        let name = doc
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        if let Some(name) = name {
            out.push((name, doc));
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    out
}

const WORKSPACE_CRATES: &[&str] = &[
    "audio", "config", "kernel", "library", "macos", "runtime", "sifr", "terminal",
    "widgets",
];

pub(crate) fn sifr_dependencies(manifest: &toml::Value, out: &mut BTreeSet<String>) {
    let Some(table) = manifest.as_table() else {
        return;
    };
    for (key, nested) in table {
        let is_dependency_table = matches!(
            key.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        );
        if is_dependency_table && let Some(dependencies) = nested.as_table() {
            for name in dependencies.keys() {
                if WORKSPACE_CRATES.contains(&name.as_str()) {
                    out.insert(name.clone());
                }
            }
        }
        sifr_dependencies(nested, out);
    }
}

pub(crate) fn allowed(allowlist: &[Allow], path: &str, pattern: &str) -> bool {
    allowlist
        .iter()
        .any(|row| row.path == path && row.pattern == pattern)
}

pub(crate) fn stale(
    allowlist: &[Allow],
    seen: &[(String, &'static str)],
) -> Vec<String> {
    allowlist
        .iter()
        .filter(|row| {
            !seen
                .iter()
                .any(|(path, pattern)| path == row.path && *pattern == row.pattern)
        })
        .map(|row| format!("{}: `{}` ({})", row.path, row.pattern, row.reason))
        .collect()
}

pub(crate) fn stale_by_text(
    allowlist: &[Allow],
    files: &[(String, PathBuf)],
) -> Vec<String> {
    let seen: Vec<(String, &'static str)> = allowlist
        .iter()
        .filter(|row| {
            files.iter().any(|(relative, path)| {
                relative == row.path && read(path).contains(row.pattern)
            })
        })
        .map(|row| (row.path.to_owned(), row.pattern))
        .collect();
    stale(allowlist, &seen)
}

pub(crate) fn report(headline: &str, violations: &[String], stale_rows: &[String]) {
    assert!(
        violations.is_empty() && stale_rows.is_empty(),
        "{headline}\n\
         {} violation(s):\n{}\n\
         {} stale allowlist row(s) — nothing matches these any more, delete them:\n{}",
        violations.len(),
        violations.join("\n"),
        stale_rows.len(),
        stale_rows.join("\n"),
    );
}

pub(crate) fn parameters(content: &str) -> Vec<(usize, String, Vec<String>)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for (index, line) in content.lines().enumerate() {
        let line_start = offset;
        offset += line.len() + 1;
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some((name, open)) = declaration(line) else {
            continue;
        };
        let Some(rest) = content.get(line_start + open..) else {
            continue;
        };
        out.push((index + 1, name, parameter_slots(rest)));
    }
    out
}

fn declaration(line: &str) -> Option<(String, usize)> {
    let start = keyword_end(line)?;
    let after_keyword = line.get(start..)?;
    let name_end = after_keyword.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    let name = after_keyword.get(..name_end)?;
    if name.is_empty() {
        return None;
    }
    let between = after_keyword.get(name_end..)?;
    let open = between.find('(')?;
    if !between
        .get(..open)?
        .chars()
        .all(|c| c.is_alphanumeric() || "<>_,' :&+".contains(c))
    {
        return None;
    }
    Some((name.to_owned(), start + name_end + open + '('.len_utf8()))
}

fn keyword_end(line: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(position) = line.get(from..)?.find("fn ") {
        let start = from + position;
        let preceded_by_word = line
            .get(..start)?
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !preceded_by_word {
            return Some(start + "fn ".len());
        }
        from = start + "fn ".len();
    }
    None
}

fn parameter_slots(rest: &str) -> Vec<String> {
    let mut depth: i32 = 0;
    let mut slots: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous = ' ';
    for c in rest.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth == 0 => {
                slots.push(current);
                return real_parameters(&slots);
            }
            ')' | ']' => depth -= 1,
            '<' if previous != '-' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            ',' if depth == 0 => {
                slots.push(std::mem::take(&mut current));
                previous = c;
                continue;
            }
            _ => {}
        }
        current.push(c);
        previous = c;
    }
    real_parameters(&slots)
}

fn real_parameters(slots: &[String]) -> Vec<String> {
    let named: Vec<String> = slots
        .iter()
        .map(|slot| slot.trim().to_owned())
        .filter(|slot| !slot.is_empty())
        .collect();
    let receiver = named.first().is_some_and(|first| first.ends_with("self"));
    named.into_iter().skip(usize::from(receiver)).collect()
}
