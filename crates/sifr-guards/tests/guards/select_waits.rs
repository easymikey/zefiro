use crate::guards::{lexer::File, support};

fn select_hits(file: &File) -> Vec<String> {
    let tests = file.test_tokens();
    (0..file.tokens.len())
        .filter(|at| {
            file.tx(*at) == "Select"
                && file.tx(at + 1) == ":"
                && file.tx(at + 2) == ":"
                && file.tx(at + 3) == "new"
                && !tests.get(*at).copied().unwrap_or(false)
        })
        .map(|at| format!("{}:{}: `Select::new()`", file.path, file.tokens[at].line))
        .collect()
}

#[test]
fn no_select_new_in_the_runtime() {
    let violations: Vec<String> = support::files_in(&["runtime"], "src")
        .iter()
        .flat_map(|(relative, path)| {
            select_hits(&File::parse(relative, &support::read(path)))
        })
        .collect();
    support::report(
        "select guard: a runtime wait is `select_biased!` with a `default(timeout)` arm, no `Select::new()`.",
        &violations,
        &[],
    );
}

const SAMPLE: &str = r"fn wait(rx: Receiver<u8>) {
    let mut select = crossbeam_channel::Select::new();
    let other = Select::new();
}
#[cfg(test)]
mod tests {
    fn g() {
        let select = Select::new();
    }
}
";

#[test]
fn select_new_is_seen_only_outside_tests() {
    let file = File::parse("runtime/src/sample.rs", SAMPLE);
    let expected = [
        "runtime/src/sample.rs:2: `Select::new()`",
        "runtime/src/sample.rs:3: `Select::new()`",
    ];
    assert_eq!(select_hits(&file), expected);
}
