// GUARD: the paint path only paints: no dispatch, driver, channel or clock.

use std::path::PathBuf;

use crate::guards::support;

fn paint_path_files() -> Vec<(String, PathBuf)> {
    const INPUT_SIDE: [&str; 2] =
        ["sifr/src/shell/input.rs", "sifr/src/shell/shell_input.rs"];
    support::files_in(&["sifr"], "src/shell")
        .into_iter()
        .filter(|(rel, _)| !INPUT_SIDE.contains(&rel.as_str()))
        .collect()
}

const DENYLIST: &[&str] = &[
    "drivers.",
    ".media.",
    "cover_events",
    "decode_cover(",
    "try_recv(",
    "SystemTime::now",
    "Instant::now",
];

fn denylist_hit(line: &str) -> Option<&'static str> {
    DENYLIST
        .iter()
        .copied()
        .find(|pattern| line.contains(pattern))
}

fn calls_update(line: &str) -> Option<&'static str> {
    ["deliver(", "update("].into_iter().find(|needle| {
        line.match_indices(needle).any(|(index, _)| {
            !index
                .checked_sub(1)
                .and_then(|before| line.as_bytes().get(before))
                .is_some_and(|byte| {
                    let previous = char::from(*byte);
                    previous.is_alphanumeric() || previous == '_'
                })
        })
    })
}

#[test]
fn update_has_one_call_site() {
    let needle = concat!("update::update", "(");
    let mut call_sites: Vec<String> = Vec::new();

    let files = support::files_in(&["runtime", "sifr"], "src");
    assert!(
        !files.is_empty(),
        "expected to find runtime and sifr sources"
    );

    for (rel, path) in files {
        let content = support::read(&path);
        for (i, raw_line) in content.lines().enumerate() {
            let trimmed = raw_line.trim_start();
            if trimmed.starts_with("//") || !raw_line.contains(needle) {
                continue;
            }
            call_sites.push(format!("{rel}:{}: {}", i + 1, trimmed));
        }
    }

    assert_eq!(
        call_sites.len(),
        1,
        "kernel::update must have one call site — found:\n{}",
        call_sites.join("\n")
    );
    let site = call_sites.first().map_or("", String::as_str);
    assert!(
        site.starts_with("runtime/src/runtime.rs:")
            && site.contains("&mut self.model, message"),
        "the sole call site should be the runtime's own `match kernel::update::update(...)` \
         line in runtime/src/runtime.rs — found: {site:?}"
    );
}

#[test]
fn paint_path_never_updates() {
    let mut violations: Vec<String> = Vec::new();

    let files = paint_path_files();
    assert!(
        !files.is_empty(),
        "expected to find the sifr shell paint sources"
    );

    for (rel, path) in &files {
        let content = support::read(path);
        let lines: Vec<&str> = content.lines().collect();

        for (i, raw_line) in lines.iter().enumerate() {
            let n = i + 1;
            let trimmed = raw_line.trim_start();
            if trimmed == "mod tests {" {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }

            let reason = calls_update(raw_line).or_else(|| denylist_hit(raw_line));
            let Some(pattern) = reason else {
                continue;
            };
            violations.push(format!(
                "{rel}:{n}: painting only paints — `{pattern}` belongs in the poll step"
            ));
        }
    }

    support::report(
        "dispatch guard: the paint path only paints — no dispatch, no driver, no clock \
         read (docs/principles.md, paint path purity).",
        &violations,
    );
}
