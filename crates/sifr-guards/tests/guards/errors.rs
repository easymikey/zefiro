// GUARD: an error carries named context: no boxed, string or swallowed error.

use std::collections::{BTreeMap, BTreeSet};

use crate::guards::support::{self, Allow};

const ALLOWLIST: &[Allow] = &[Allow::new(
    "terminal/src/window_colors.rs",
    "let _ = stdout.write_all(",
    "emit()'s own doc: raw terminal escape-sequence writes are best-effort, \
         same as the teardown calls.",
)];

fn allowed(path: &str, line: &str) -> bool {
    ALLOWLIST
        .iter()
        .any(|row| row.path == path && line.contains(row.pattern))
}

fn test_line_flags(lines: &[&str]) -> Vec<bool> {
    let mut flags = Vec::with_capacity(lines.len());
    let mut depth: i32 = 0;
    let mut pending = false;
    let mut scope_end_depth: Option<i32> = None;
    for line in lines {
        flags.push(scope_end_depth.is_some());
        if line.contains("#[cfg(test)]") {
            pending = true;
        }
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    if pending && scope_end_depth.is_none() {
                        scope_end_depth = Some(depth - 1);
                        pending = false;
                    }
                }
                '}' => {
                    depth -= 1;
                    if let Some(end) = scope_end_depth
                        && depth <= end
                    {
                        scope_end_depth = None;
                    }
                }
                _ => {}
            }
        }
    }
    flags
}

fn ident_before_paren(text: &str, open: usize) -> &str {
    let before = &text[..open];
    let start = before
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '!'))
        .map_or(0, |p| p + 1);
    before[start..].trim_end_matches('!')
}

fn has_stringified_ctor(line: &str) -> bool {
    for (open, _) in line.match_indices('(') {
        let ident = ident_before_paren(line, open);
        if ident.ends_with("Error")
            || ident.ends_with("Failed")
            || ident.ends_with("Fault")
        {
            let rest = &line[open + 1..];
            if let Some(close) = rest.find(')')
                && rest[..=close].ends_with(".to_string()")
            {
                return true;
            }
        }
    }
    false
}

fn has_swallowed_write(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("let _ = ") else {
        return false;
    };
    let Some(open) = rest.find('(') else {
        return false;
    };
    let ident = ident_before_paren(rest, open);
    if ident == "send" {
        return false;
    }
    ["save", "append", "write", "move_to_trash", "persist"]
        .iter()
        .any(|verb| ident.contains(verb))
}

fn enum_name(line: &str) -> Option<&str> {
    let trimmed = line
        .trim_start()
        .strip_prefix("pub ")
        .unwrap_or_else(|| line.trim_start());
    let after = trimmed.strip_prefix("enum ")?;
    let end = after
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    Some(&after[..end])
}

fn crate_of_rel(rel: &str) -> &str {
    rel.split('/').next().unwrap_or(rel)
}

fn crate_deps() -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for (crate_name, doc) in support::manifests() {
        let mut deps = BTreeSet::new();
        support::sifr_dependencies(&doc, &mut deps);
        out.insert(crate_name, deps);
    }
    out
}

fn pub_error_enum_name(line: &str) -> Option<&str> {
    let after = line.trim_start().strip_prefix("pub enum ")?;
    let end = after
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    let name = &after[..end];
    (name.ends_with("Error") || name.ends_with("Fault")).then_some(name)
}

fn variant_ident(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return None;
    }
    let end = trimmed.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    if end == 0 {
        return None;
    }
    let ident = &trimmed[..end];
    let after = trimmed[end..].trim_start();
    (after.starts_with('{') || after.starts_with('(') || after.starts_with(','))
        .then_some(ident)
}

fn enum_body_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0i32;
    let mut started = false;
    let mut end = start;
    for (offset, line) in lines.iter().enumerate().skip(start) {
        end = offset;
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if started && depth == 0 {
            break;
        }
    }
    end
}

fn bare_ident(raw: &str) -> &str {
    let mut s = raw.trim().trim_start_matches('&').trim_start();
    if let Some(rest) = s.strip_prefix('\'') {
        s = rest
            .find(char::is_whitespace)
            .map_or("", |sp| rest[sp..].trim_start());
    }
    let s = s.rsplit("::").next().unwrap_or(s);
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    &s[..end]
}

fn from_impl_pair(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("impl") {
        return None;
    }
    let after_from = trimmed.split_once("From<")?.1;
    let (x_raw, rest) = after_from.split_once('>')?;
    let after_for = rest.split_once(" for ")?.1.trim_start();
    let y_end = after_for
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(after_for.len());
    Some((bare_ident(x_raw), &after_for[..y_end]))
}

fn from_field_type(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return None;
    }
    let after = line.split_once("#[from]")?.1.trim_start();
    let end = after.find([')', ',']).unwrap_or(after.len());
    Some(bare_ident(&after[..end]))
}

fn resolve_enum<'a>(
    enum_crate: &'a BTreeMap<String, BTreeSet<String>>,
    name: &str,
    from: &str,
) -> Option<&'a str> {
    let crates = enum_crate.get(name)?;
    if let Some(own) = crates.get(from) {
        return Some(own);
    }
    let mut all = crates.iter();
    match (all.next(), all.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

struct Edge<'a> {
    rel: &'a str,
    line_idx: usize,
    y_name: &'a str,
    x_name: &'a str,
    line: &'a str,
}

fn check_from_edge(
    edge: &Edge<'_>,
    enum_crate: &BTreeMap<String, BTreeSet<String>>,
    deps: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    let Edge {
        rel,
        line_idx,
        y_name,
        x_name,
        line,
    } = edge;
    let c = crate_of_rel(rel);
    let (Some(y_crate), Some(x_crate)) = (
        resolve_enum(enum_crate, y_name, c),
        resolve_enum(enum_crate, x_name, c),
    ) else {
        return Vec::new();
    };
    let visible = |crate_name: &str| {
        crate_name == c || deps.get(c).is_some_and(|d| d.contains(crate_name))
    };
    if !(visible(y_crate) && visible(x_crate)) && !allowed(rel, line) {
        let n = *line_idx + 1;
        return vec![format!(
            "{rel}:{n}: `{y_name}: From<{x_name}>` written in {c}, which cannot see both \
             {x_name} ({x_crate}) and {y_name} ({y_crate}) via its Cargo.toml dependencies"
        )];
    }
    Vec::new()
}

#[test]
fn every_error_variant_has_error_attr_and_from_follows_dependency_graph() {
    let mut violations: Vec<String> = Vec::new();

    let files = support::source_files(&["src"]);
    let mut enum_crate: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (rel, path) in &files {
        let content = support::read(path);
        for line in content.lines() {
            if let Some(name) = pub_error_enum_name(line) {
                enum_crate
                    .entry(name.to_owned())
                    .or_default()
                    .insert(crate_of_rel(rel).to_owned());
            }
        }
    }
    let deps = crate_deps();

    for (rel, path) in &files {
        let content = support::read(path);
        let lines: Vec<&str> = content.lines().collect();

        let mut i = 0;
        while let Some(head) = lines.get(i) {
            if let Some(name) = pub_error_enum_name(head) {
                let j = enum_body_end(&lines, i);
                for (k, line) in lines.iter().enumerate().take(j + 1).skip(i) {
                    if let Some(variant) = variant_ident(line) {
                        let has_attr = k
                            .checked_sub(1)
                            .and_then(|previous| lines.get(previous))
                            .is_some_and(|previous| previous.contains("#[error("));
                        if !has_attr && !allowed(rel, line) {
                            violations.push(format!(
                                "{rel}:{name}::{variant}: variant missing a preceding \
                                 #[error(\"...\")] attribute"
                            ));
                        }
                    }
                    if let Some(x) = from_field_type(line) {
                        violations.extend(check_from_edge(
                            &Edge {
                                rel,
                                line_idx: k,
                                y_name: name,
                                x_name: x,
                                line,
                            },
                            &enum_crate,
                            &deps,
                        ));
                    }
                }
                i = j;
            }
            i += 1;
        }

        for (i, line) in lines.iter().enumerate() {
            if let Some((x, y)) = from_impl_pair(line) {
                violations.extend(check_from_edge(
                    &Edge {
                        rel,
                        line_idx: i,
                        y_name: y,
                        x_name: x,
                        line,
                    },
                    &enum_crate,
                    &deps,
                ));
            }
        }
    }

    support::report(
        "errors guard: every error variant carries #[error(\"...\")], and every \
         From/#[from] between two of our error enums follows a real Cargo.toml edge \
         (docs/errors.md).",
        &violations,
        &[],
    );
}

const SIMPLE_RULES: &[support::Rule] = &[
    support::Rule {
        pattern: "Box<dyn Error",
        message: "Box<dyn Error> — box the concrete error type instead",
    },
    support::Rule {
        pattern: "Box<dyn std::error::Error",
        message: "Box<dyn Error> — box the concrete error type instead",
    },
];

fn check_simple_rules(rel: &str, lines: &[&str], violations: &mut Vec<String>) {
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        if let Some(rule) = support::rule_hit(line, SIMPLE_RULES)
            && !allowed(rel, line)
        {
            violations.push(format!("{rel}:{n}: {}", rule.message));
        }
    }
}

fn check_bare_string_payload(rel: &str, lines: &[&str], violations: &mut Vec<String>) {
    let mut i = 0;
    while let Some(head) = lines.get(i) {
        if let Some(name) = enum_name(head)
            && (name.ends_with("Error") || name.ends_with("Fault"))
        {
            let j = enum_body_end(lines, i);
            for (k, body_line) in lines.iter().enumerate().take(j + 1).skip(i) {
                let n = k + 1;
                if body_line.replace(' ', "").contains("(String)")
                    && !allowed(rel, body_line)
                {
                    violations.push(format!(
                        "{rel}:{n}: bare (String) payload on `{name}` — use a named context field instead"
                    ));
                }
            }
            i = j;
        }
        i += 1;
    }
}

fn check_stringified_ctor(rel: &str, lines: &[&str], violations: &mut Vec<String>) {
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        if has_stringified_ctor(line) && !allowed(rel, line) {
            violations.push(format!(
                "{rel}:{n}: .to_string() passed into an Error/Failed/Fault constructor — keep the source typed"
            ));
        }
    }
}

struct SourceLines<'a> {
    lines: &'a [&'a str],
    in_test: &'a [bool],
}

fn check_swallowed_write(
    rel: &str,
    source: &SourceLines<'_>,
    violations: &mut Vec<String>,
) {
    for (i, (line, in_test)) in source.lines.iter().zip(source.in_test).enumerate() {
        let n = i + 1;
        if !*in_test && has_swallowed_write(line) && !allowed(rel, line) {
            violations.push(format!(
                "{rel}:{n}: `let _ =` swallows a save/append/write/move_to_trash/persist call outside tests"
            ));
        }
    }
}

#[test]
fn no_boxed_or_string_payload_errors_and_no_swallowed_writes() {
    let mut violations: Vec<String> = Vec::new();

    let files = support::source_files(&["src"]);
    for (rel, path) in &files {
        let content = support::read(path);
        let lines: Vec<&str> = content.lines().collect();
        let in_test = test_line_flags(&lines);

        check_simple_rules(rel, &lines, &mut violations);
        check_bare_string_payload(rel, &lines, &mut violations);
        check_stringified_ctor(rel, &lines, &mut violations);
        check_swallowed_write(
            rel,
            &SourceLines {
                lines: &lines,
                in_test: &in_test,
            },
            &mut violations,
        );
    }

    support::report(
        "errors guard: no Box<dyn Error>, no bare (String) payload, no .to_string() \
         into an error constructor, no `let _ =` over a write (docs/errors.md).",
        &violations,
        &support::stale_by_text(ALLOWLIST, &files),
    );
}
