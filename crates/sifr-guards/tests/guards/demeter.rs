// GUARD: below the router, no `kernel` update handler holds a whole `Model`.

use crate::guards::support::{self, Allow};

const ALLOWED_MODEL_FUNCTIONS: &[Allow] = &[Allow::new(
    "settings.rs",
    "step_setting",
    "routes a SettingRow nudge across themes, settings and appearance",
)];

const EXEMPT_FILES: &[&str] = &["mod.rs", "parts.rs", "startup.rs"];

const UPDATE_TREE: &str = "kernel/src/update/";

fn line_reaches_into_model(line: &str) -> bool {
    reaches_by_ref(line) || dots_into_model(line)
}

fn reaches_by_ref(line: &str) -> bool {
    let mut search_from = 0;
    while let Some(pos) = line[search_from..].find("Model") {
        let start = search_from + pos;
        let end = start + "Model".len();
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
            if before.ends_with('&') || before.ends_with("&mut") {
                return true;
            }
        }
        search_from = end;
    }
    false
}

fn dots_into_model(line: &str) -> bool {
    let mut search_from = 0;
    while let Some(pos) = line[search_from..].find("model.") {
        let start = search_from + pos;
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

fn fn_name_declared(trimmed: &str) -> Option<&str> {
    let after_fn = trimmed.split_once("fn ")?.1;
    let name_end = after_fn.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    let name = &after_fn[..name_end];
    (!name.is_empty()).then_some(name)
}

#[test]
fn update_handlers_below_the_router_take_their_slices_not_a_whole_model() {
    let files = support::files_in(&["kernel"], "src/update");
    assert!(
        !files.is_empty(),
        "expected to find kernel src/update sources"
    );

    let mut violations = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    for (keyed, path) in &files {
        let Some(relative_str) = keyed.strip_prefix(UPDATE_TREE).map(str::to_owned)
        else {
            continue;
        };

        if EXEMPT_FILES.contains(&relative_str.as_str()) {
            continue;
        }

        let content = support::read(path);

        let mut in_allowed_fn = false;
        let mut fn_depth: i32 = 0;
        let mut fn_opened = false;

        for (i, raw_line) in content.lines().enumerate() {
            let trimmed = raw_line.trim_start();
            if trimmed == "#[cfg(test)]" {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }

            if !in_allowed_fn
                && let Some(name) = fn_name_declared(trimmed)
                && let Some(row) = ALLOWED_MODEL_FUNCTIONS
                    .iter()
                    .find(|row| row.path == relative_str && row.pattern == name)
            {
                in_allowed_fn = true;
                fn_depth = 0;
                fn_opened = false;
                seen.push((relative_str.clone(), row.pattern));
            }

            if in_allowed_fn {
                for ch in raw_line.chars() {
                    match ch {
                        '{' => {
                            fn_depth += 1;
                            fn_opened = true;
                        }
                        '}' => fn_depth -= 1,
                        _ => {}
                    }
                }
                if fn_opened && fn_depth <= 0 {
                    in_allowed_fn = false;
                }
                continue;
            }

            if line_reaches_into_model(raw_line) {
                violations.push(format!(
                    "{}:{}: {}",
                    relative_str,
                    i + 1,
                    raw_line.trim()
                ));
            }
        }
    }

    support::report_with_stale(
        "demeter guard: update handlers below update/mod.rs, update/parts.rs and update/startup.rs take \
         only the Model slices they touch — a handler that genuinely spans 4+ slices, \
         or forwards the whole Model to one that does, belongs in \
         ALLOWED_MODEL_FUNCTIONS instead.",
        &violations,
        &support::stale(ALLOWED_MODEL_FUNCTIONS, &seen),
    );
}
