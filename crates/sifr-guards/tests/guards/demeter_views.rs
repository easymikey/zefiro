// GUARD: below `screen`, no `widgets` component holds a whole `Model` — each
// takes the narrow view it reads.

use std::path::PathBuf;

use crate::guards::support::{self, Allow};

const RENDER_SOURCE: &str = "widgets/src/";

const ALLOWED_FROM_MODEL_FILES: &[&str] = &[
    "components/organisms/card/mod.rs",
    "components/organisms/key_hints.rs",
    "components/organisms/playlist.rs",
];

const SCREEN_ROOTS: &[&str] = &["screen/mod.rs"];

fn is_screen_root(relative: &str) -> bool {
    SCREEN_ROOTS.contains(&relative)
}

fn render_sources() -> Vec<(String, PathBuf)> {
    support::files_in(&["widgets"], "src")
        .into_iter()
        .filter_map(|(keyed, path)| {
            keyed
                .strip_prefix(RENDER_SOURCE)
                .map(|relative| (relative.to_owned(), path))
        })
        .collect()
}

fn line_reaches_into_model(line: &str) -> bool {
    reaches_by_ref_type(line, "Model") || dots_into_model(line)
}

fn dots_into_model(line: &str) -> bool {
    let mut search_from = 0;
    while let Some(position) = line[search_from..].find("model.") {
        let start = search_from + position;
        let word_boundary_before = start == 0
            || line[..start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric() && c != '_');
        if word_boundary_before {
            return true;
        }
        search_from = start + "model.".len();
    }
    false
}

fn reaches_by_ref_type(line: &str, type_name: &str) -> bool {
    let mut search_from = 0;
    while let Some(position) = line[search_from..].find(type_name) {
        let start = search_from + position;
        let end = start + type_name.len();
        let word_boundary_after = line[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if word_boundary_after {
            let before = line[..start].trim_end();
            let before = match before.rfind('\'') {
                Some(tick) if before[tick + 1..].chars().all(char::is_alphanumeric) => {
                    before[..tick].trim_end()
                }
                _ => before,
            };
            if before.ends_with('&') {
                return true;
            }
        }
        search_from = end;
    }
    false
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    for ch in line.chars() {
        match ch {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

#[test]
fn organisms_and_below_never_reach_a_whole_model_outside_from_model() {
    let files = render_sources();
    assert!(!files.is_empty(), "expected to find widgets sources");

    let mut violations = Vec::new();

    for (relative, path) in &files {
        if is_screen_root(relative) {
            continue;
        }
        let allowlisted = ALLOWED_FROM_MODEL_FILES.contains(&relative.as_str());
        let content = support::read(path);

        let mut in_constructor = false;
        let mut constructor_depth: i32 = 0;

        for (i, raw_line) in content.lines().enumerate() {
            let trimmed = raw_line.trim_start();
            if trimmed == "#[cfg(test)]" {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }

            if !in_constructor && allowlisted && trimmed.contains("fn from_model") {
                in_constructor = true;
                constructor_depth = 0;
            }

            if in_constructor {
                constructor_depth += brace_delta(raw_line);
                if constructor_depth <= 0 {
                    in_constructor = false;
                }
            } else if line_reaches_into_model(raw_line) {
                violations.push(format!("{relative}:{}: {}", i + 1, raw_line.trim()));
            }
        }
    }

    support::report(
        "demeter (views) guard: components below screen/** take a narrow view slice \
         built by an allowlisted `from_model` constructor, never a whole &Model.",
        &violations,
        &[],
    );
}

const UI_ALLOWLIST: &[Allow] = &[Allow::new(
    "overlay/layer.rs",
    "pub(crate) workspace: &'a Workspace,",
    "OverlayView's own `workspace` field and the `OverlayFrame` that builds it — overlays \
     genuinely need overlay state + theme; allowlisted by name, not narrowed.",
)];

const SLICE_ALLOWLIST: &[Allow] = &[];

const CHAIN_ALLOWLIST: &[Allow] = &[];

const SLICE_TYPES: &[&str] =
    &["Playlist", "Library", "Transport", "Settings", "History"];

fn chains_deeper_than_one_field_off_workspace(line: &str) -> bool {
    for prefix in ["context.workspace.", "ctx.workspace."] {
        let mut search_from = 0;
        while let Some(position) = line[search_from..].find(prefix) {
            let start = search_from + position;
            let after = &line[start + prefix.len()..];
            let field_end = after
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(after.len());
            if after[field_end..].starts_with('.') {
                return true;
            }
            search_from = start + prefix.len();
        }
    }
    false
}

fn scan_scoped_lines(
    in_scope: impl Fn(&str) -> bool,
    mut check: impl FnMut(&str, usize, &str),
) {
    let files = render_sources();
    assert!(!files.is_empty(), "expected to find widgets sources");

    for (relative, path) in &files {
        if !in_scope(relative) {
            continue;
        }
        let content = support::read(path);
        for (i, raw_line) in content.lines().enumerate() {
            let trimmed = raw_line.trim_start();
            if trimmed == "#[cfg(test)]" {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            check(relative, i + 1, raw_line);
        }
    }
}

fn excuse(
    allowlist: &'static [Allow],
    relative: &str,
    raw_line: &str,
) -> Option<&'static Allow> {
    allowlist
        .iter()
        .find(|row| row.path == relative && raw_line.contains(row.pattern))
}

#[test]
fn organisms_and_below_never_hold_a_bare_workspace_outside_overlay_context() {
    let mut violations = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    scan_scoped_lines(
        |relative| !is_screen_root(relative),
        |relative, line_no, raw_line| {
            if !reaches_by_ref_type(raw_line, "Workspace") {
                return;
            }
            match excuse(UI_ALLOWLIST, relative, raw_line) {
                Some(row) => seen.push((relative.to_owned(), row.pattern)),
                None => violations
                    .push(format!("{relative}:{line_no}: {}", raw_line.trim())),
            }
        },
    );

    support::report(
        "demeter (views) guard: components take the specific `Workspace` field \
         they read, not a bare `&Workspace` — only `OverlayView` is allowlisted for that.",
        &violations,
        &support::stale(UI_ALLOWLIST, &seen),
    );
}

#[test]
fn card_compact_card_and_minimal_never_hold_a_whole_domain_slice() {
    let mut violations = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    scan_scoped_lines(
        |relative| {
            relative.starts_with("components/organisms/card/")
                || relative == "components/organisms/compact_card.rs"
                || relative == "screen/minimal.rs"
        },
        |relative, line_no, raw_line| {
            let hits_a_slice_type = SLICE_TYPES
                .iter()
                .any(|name| reaches_by_ref_type(raw_line, name));
            if !hits_a_slice_type {
                return;
            }
            match excuse(SLICE_ALLOWLIST, relative, raw_line) {
                Some(row) => seen.push((relative.to_owned(), row.pattern)),
                None => violations
                    .push(format!("{relative}:{line_no}: {}", raw_line.trim())),
            }
        },
    );

    support::report(
        "demeter (views) guard: card/compact_card/minimal row helpers take only the \
         fields they read off Playlist/Library/Transport/Settings/History, not the \
         whole slice by reference.",
        &violations,
        &support::stale(SLICE_ALLOWLIST, &seen),
    );
}

#[test]
fn overlay_workspace_chains_never_go_deeper_than_one_field() {
    let mut violations = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    scan_scoped_lines(
        |relative| relative.starts_with("components/organisms/overlays/"),
        |relative, line_no, raw_line| {
            if !chains_deeper_than_one_field_off_workspace(raw_line) {
                return;
            }
            match excuse(CHAIN_ALLOWLIST, relative, raw_line) {
                Some(row) => seen.push((relative.to_owned(), row.pattern)),
                None => violations
                    .push(format!("{relative}:{line_no}: {}", raw_line.trim())),
            }
        },
    );

    support::report(
        "demeter (views) guard: an overlay chains at most one field off `workspace` (the \
         `workspace.overlay` convention every overlay's own `render` uses).",
        &violations,
        &support::stale(CHAIN_ALLOWLIST, &seen),
    );
}
