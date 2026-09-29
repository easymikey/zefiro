// GUARD: no `macro_rules!` and no proc-macro crate of our own — a generic type
// or a derive from std/serde/strum does the job while staying visible to the
// reader.

use crate::guards::support::{self, Allow};

const DECLARATIONS: &[&str] = &[
    concat!("macro_", "rules!"),
    "proc_macro_derive",
    "proc_macro_attribute",
    "#[proc_macro]",
];

const ALLOWLIST: &[Allow] = &[];

#[test]
fn no_declarative_or_proc_macros_anywhere() {
    let mut violations: Vec<String> = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    let files = support::source_files(&["src", "tests", "benches", "examples"]);
    assert!(
        !files.is_empty(),
        "expected to find the workspace's sources"
    );

    for (rel, path) in files {
        if rel.starts_with("sifr-guards/") {
            continue;
        }
        let content = support::read(&path);
        for (index, raw_line) in content.lines().enumerate() {
            if raw_line.trim_start().starts_with("//") {
                continue;
            }
            for &declaration in DECLARATIONS {
                if !raw_line.contains(declaration) {
                    continue;
                }
                if support::allowed(ALLOWLIST, &rel, declaration) {
                    seen.push((rel.clone(), declaration));
                } else {
                    violations.push(format!(
                        "{rel}:{}: declares `{declaration}` — a generic type or a derive says \
                         the same thing where the compiler and the reader can see it",
                        index + 1
                    ));
                }
            }
        }
    }

    support::report(
        "macros guard: the workspace declares no declarative or proc macros (cleanup plan §2).",
        &violations,
        &support::stale(ALLOWLIST, &seen),
    );
}
