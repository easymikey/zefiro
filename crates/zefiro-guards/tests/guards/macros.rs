// GUARD: no `macro_rules!` and no proc-macro crate of our own.

use crate::guards::support;

const DECLARATIONS: &[&str] = &[
    concat!("macro_", "rules!"),
    "proc_macro_derive",
    "proc_macro_attribute",
    "#[proc_macro]",
];

#[test]
fn no_declarative_or_proc_macros_anywhere() {
    let mut violations: Vec<String> = Vec::new();

    let files = support::source_files(&["src", "tests", "benches", "examples"]);
    assert!(
        !files.is_empty(),
        "expected to find the workspace's sources"
    );

    for (rel, path) in files {
        if rel.starts_with("zefiro-guards/") {
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
                violations.push(format!(
                    "{rel}:{}: declares `{declaration}` — a generic type or a derive says \
                     the same thing where the compiler and the reader can see it",
                    index + 1
                ));
            }
        }
    }

    support::report(
        "macros guard: the workspace declares no declarative or proc macros (cleanup plan §2).",
        &violations,
    );
}
