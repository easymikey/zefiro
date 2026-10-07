// GUARD: no file under `crates/**` runs past 800 lines.

use crate::guards::support;

const LIMIT: usize = 800;

#[test]
fn no_file_runs_past_six_hundred_lines() {
    let mut violations: Vec<String> = Vec::new();

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
        violations.push(format!(
            "{rel}: {lines} lines, over the {LIMIT}-line limit — split it into the \
             things it is doing"
        ));
    }

    support::report(
        "length guard: no file in crates/** runs past 800 lines, tests and benches \
         included (cleanup plan §3).",
        &violations,
    );
}
