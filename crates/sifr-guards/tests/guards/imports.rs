// GUARD: every `use` starts at `crate::` or an external crate, no glob.

use crate::guards::support::{self, Allow};

const ALLOWLIST: &[Allow] = &[];

const PARENT_SEGMENT: &str = concat!("super", "::");
const GLOB_SEGMENT: &str = concat!("::", "*");

const PARENT_RULE: &str = "parent-relative import";
const GLOB_RULE: &str = "glob import";

fn starts_use_statement(line: &str) -> bool {
    let rest = line.strip_prefix("pub").map_or(line, |after| {
        after
            .split_once(char::is_whitespace)
            .map_or(after, |(_visibility, tail)| tail)
            .trim_start()
    });
    rest.starts_with("use ") || rest.starts_with("use\t")
}

fn broken_rule(line: &str) -> Option<&'static str> {
    if line.contains(PARENT_SEGMENT) {
        return Some(PARENT_RULE);
    }
    if line.contains(GLOB_SEGMENT) {
        return Some(GLOB_RULE);
    }
    None
}

fn offenders(content: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    let mut inside_use = false;
    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        if !inside_use {
            if !starts_use_statement(line) {
                continue;
            }
            inside_use = true;
        }
        if let Some(rule) = broken_rule(line) {
            found.push((index + 1, rule));
        }
        if line.contains(';') {
            inside_use = false;
        }
    }
    found
}

#[test]
fn every_use_names_an_absolute_path_and_no_glob() {
    let mut violations: Vec<String> = Vec::new();
    let mut seen_allowlist: Vec<(String, &'static str)> = Vec::new();

    let files = support::source_files(&["src", "tests", "examples"]);
    assert!(
        files.len() > 100,
        "expected to find the workspace's sources, found {}",
        files.len()
    );

    for (rel, path) in files {
        let content = support::read(&path);
        for (line, rule) in offenders(&content) {
            if support::allowed(ALLOWLIST, &rel, rule) {
                seen_allowlist.push((rel.clone(), rule));
            } else {
                violations.push(format!("{rel}:{line}: {rule}"));
            }
        }
    }

    support::report(
        "import guard: every `use` names its path from `crate::` (or an external \
         crate) and no `use` ends in a glob — see docs/agent-rules.md.",
        &violations,
        &support::stale(ALLOWLIST, &seen_allowlist),
    );
}
