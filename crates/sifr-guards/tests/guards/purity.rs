// GUARD: `kernel`, `widgets` are pure: no IO, clock or threads.

use crate::guards::support;

const PURE_CRATES: &[&str] = &["kernel", "widgets"];

const DENYLIST: &[&str] = &[
    "std::fs",
    "std::io",
    "std::thread",
    "std::time::Instant",
    "SystemTime",
    "std::env",
    "std::process",
    "std::net",
    "tokio",
    "crossbeam",
    "dirs::",
    "notify",
    "println!",
    "eprintln!",
    "dbg!",
];

const PERMITTED: &str = "std::io::ErrorKind";

fn denylist_hit(line: &str) -> Option<&'static str> {
    let scanned = line.replace(PERMITTED, "");
    DENYLIST
        .iter()
        .copied()
        .find(|pattern| scanned.contains(pattern))
}

#[test]
fn no_io_clock_threads_or_env_in_the_pure_crates() {
    let mut violations: Vec<String> = Vec::new();

    let files = support::files_in(PURE_CRATES, "src");
    assert!(
        files.len() > PURE_CRATES.len(),
        "expected to find every pure crate's sources"
    );

    for (rel, path) in files {
        let content = support::read(&path);

        for (i, raw_line) in content.lines().enumerate() {
            let n = i + 1;
            let trimmed = raw_line.trim_start();
            if trimmed == "#[cfg(test)]" {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }

            if let Some(pattern) = denylist_hit(raw_line) {
                violations.push(format!(
                    "{rel}:{n}: forbidden `{pattern}` (functional core must not touch IO/clock/threads/env)"
                ));
            }
        }
    }

    support::report(
        "purity guard: the functional core touches no IO, clock, thread or \
         environment (docs/principles.md Level 3).",
        &violations,
    );
}
