// GUARD: no file under `crates/**` runs past 800 lines.

use crate::guards::support::{self, Allow};

const LIMIT: usize = 800;

const ALLOWLIST: &[Allow] = &[];

fn ceiling(row: &Allow) -> usize {
    row.pattern.parse().unwrap_or(LIMIT)
}

#[test]
fn no_file_runs_past_six_hundred_lines() {
    let mut violations: Vec<String> = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    let files = support::source_files(&["src", "tests", "benches", "examples"]);
    assert!(
        !files.is_empty(),
        "expected to find the workspace's sources"
    );

    for (rel, path) in files {
        let lines = support::read(&path).lines().count();
        if lines <= LIMIT {
            continue;
        }
        let Some(row) = ALLOWLIST.iter().find(|row| row.path == rel) else {
            violations.push(format!(
                "{rel}: {lines} lines, over the {LIMIT}-line limit — split it into the \
                 things it is doing"
            ));
            continue;
        };
        seen.push((rel.clone(), row.pattern));
        let ceiling = ceiling(row);
        if lines > ceiling {
            violations.push(format!(
                "{rel}: {lines} lines, over its own {ceiling}-line ceiling — an \
                 over-limit file may only shrink"
            ));
        }
    }

    support::report(
        "length guard: no file in crates/** runs past 800 lines, tests and benches \
         included (cleanup plan §3).",
        &violations,
        &support::stale(ALLOWLIST, &seen),
    );
}
