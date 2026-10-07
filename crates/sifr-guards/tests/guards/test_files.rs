use crate::guards::{lexer::File, support};

const CFG_TEST: [&str; 6] = ["[", "cfg", "(", "test", ")", "]"];
const PATH: [&str; 6] = ["#", "[", "path", "=", "\"\"", "]"];

fn shape_at(file: &File, at: usize, shape: &[&str]) -> bool {
    let mut texts = shape.iter().enumerate();
    texts.all(|(offset, text)| file.tx(at + offset) == *text)
}

fn declared_mod(file: &File, from: usize) -> Option<(usize, Option<usize>)> {
    let path = shape_at(file, from, &PATH).then_some(from + 2);
    let mut at = from + path.map_or(0, |_| PATH.len());
    while matches!(file.tx(at), "pub" | "(" | "crate" | ")") {
        at += 1;
    }
    (file.tx(at) == "mod" && file.tx(at + 2) == ";").then_some((at + 1, path))
}

fn under_tests(file: &File, text: &str, path: usize) -> bool {
    let line = text.lines().nth(file.tokens[path].line - 1).unwrap_or("");
    line.split('"')
        .nth(1)
        .is_some_and(|target| target.contains("tests/"))
}

fn test_only_files(file: &File, text: &str) -> Vec<String> {
    let inner = shape_at(file, 0, &["#", "!"]) && shape_at(file, 2, &CFG_TEST);
    let whole =
        inner.then(|| format!("{}: the whole file is `#![cfg(test)]`", file.path));
    let declared = (0..file.tokens.len())
        .filter(|at| file.tx(*at) == "#" && shape_at(file, at + 1, &CFG_TEST))
        .filter_map(|at| declared_mod(file, at + 1 + CFG_TEST.len()))
        .filter(|(_, path)| !path.is_some_and(|path| under_tests(file, text, path)))
        .map(|(name, _)| {
            let line = file.tokens[name].line;
            let module = file.tx(name);
            format!("{}:{line}: `mod {module};` is a test-only file", file.path)
        });
    whole.into_iter().chain(declared).collect()
}

#[test]
fn no_test_only_source_files() {
    let check = |(relative, path): &(String, std::path::PathBuf)| {
        let text = support::read(path);
        test_only_files(&File::parse(relative, &text), &text)
    };
    let violations: Vec<String> = support::source_files(&["src"])
        .iter()
        .flat_map(check)
        .collect();
    support::report(
        "test-only file guard: tests stay in `mod tests` beside their code.",
        &violations,
    );
}

#[test]
fn test_only_file_is_a_cfg_test_mod_declaration() {
    let source = "#![cfg(test)]\n#[cfg(test)] mod support;\n\
                  #[cfg(test)]\nmod tests {}\n#[cfg(test)] pub(crate) mod fixture;\n\
                  #[cfg(test)]\n#[path = \"../tests/unit/fixtures.rs\"]\nmod outside;\n\
                  #[cfg(test)]\n#[path = \"inner.rs\"]\nmod inside;\n";
    let file = File::parse("widgets/src/lib.rs", source);
    let expected = [
        "widgets/src/lib.rs: the whole file is `#![cfg(test)]`",
        "widgets/src/lib.rs:2: `mod support;` is a test-only file",
        "widgets/src/lib.rs:5: `mod fixture;` is a test-only file",
        "widgets/src/lib.rs:11: `mod inside;` is a test-only file",
    ];
    assert_eq!(test_only_files(&file, source), expected);
}
