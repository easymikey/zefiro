use std::collections::BTreeSet;

use crate::guards::{lexer::File, support};

struct Types {
    structs: BTreeSet<String>,
    enums: BTreeSet<String>,
    crates: BTreeSet<String>,
}

fn types(files: &[File]) -> Types {
    let named = |kind: &str| {
        let sites = files.iter().flat_map(|file| file.items.iter());
        sites
            .filter(|site| site.kind == kind)
            .map(|site| site.name.clone())
            .collect()
    };
    Types {
        structs: named("struct"),
        enums: named("enum"),
        crates: files
            .iter()
            .filter_map(|file| file.path.split('/').next())
            .map(|name| name.replace('-', "_"))
            .collect(),
    }
}

fn path_root(file: &File, from: usize) -> &str {
    let mut at = from;
    while at >= 3 && file.tx(at - 1) == ":" && file.tx(at - 2) == ":" {
        at -= 3;
    }
    file.tx(at)
}

fn opening(file: &File, inner: usize) -> Option<usize> {
    let mut depth = 0_i32;
    (0..inner).rev().find(|at| {
        depth += match file.tx(*at) {
            ")" | "]" | "}" => 1,
            "(" | "[" | "{" => -1,
            _ => 0,
        };
        depth < 0
    })
}

fn own_path(file: &File, types: &Types, open: usize) -> Option<String> {
    let name = file.tx(open.checked_sub(1)?);
    let qualified = open > 3 && file.tx(open - 2) == ":" && file.tx(open - 3) == ":";
    let owner = if qualified { file.tx(open - 4) } else { "" };
    let root = if qualified {
        path_root(file, open - 4)
    } else {
        name
    };
    let workspace = root == owner
        || matches!(root, "crate" | "self" | "Self")
        || types.crates.contains(root);
    let own = name == "Self"
        || owner == "Self"
        || types.structs.contains(name)
        || (workspace && types.enums.contains(owner));
    own.then(|| {
        if qualified {
            format!("{owner}::{name}")
        } else {
            name.to_owned()
        }
    })
}

fn binds(file: &File, open: usize) -> bool {
    let close = file.matching_close(open);
    (open + 1..close).any(|at| {
        let word = file.tx(at);
        let lower =
            word.starts_with(|first: char| first.is_lowercase() || first == '_');
        let path = file.tx(at - 1) == ":" && file.tx(at - 2) == ":";
        lower
            && word != "_"
            && !matches!(word, "ref" | "mut" | "box" | "true" | "false")
            && !matches!(file.tx(at + 1), ":" | "(" | "{" | "!")
            && !path
    })
}

fn refused(file: &File, at: usize) -> bool {
    if matches!(file.tx(at.saturating_sub(1)), "if" | "while" | "&") {
        return false;
    }
    let mut next = at;
    while next < file.tokens.len() {
        match file.tx(next) {
            "(" | "[" | "{" => next = file.matching_close(next),
            ";" => return true,
            "else" if file.tx(next - 1) != "}" => return false,
            _ => {}
        }
        next += 1;
    }
    true
}

fn whole(file: &File, open: usize) -> bool {
    let mut depth = 0_i32;
    for at in (1..open).rev() {
        let before = file.tx(at - 1);
        let head = (0..at)
            .rev()
            .map(|back| file.tx(back))
            .take_while(|back| !matches!(*back, "{" | "}" | ";" | "(" | ")" | "="));
        match file.tx(at) {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            "(" if before == "!" => return false,
            "(" if head.clone().any(|back| back == "fn") => return true,
            "{" if !before.starts_with(char::is_uppercase)
                || head.clone().any(|back| back == "match") =>
            {
                return false;
            }
            "let" => return refused(file, at),
            "for" | ";" => return true,
            ">" if before == "=" => return false,
            "|" if matches!(
                before,
                "(" | "," | "=" | "{" | ";" | ">" | "[" | "|" | "move" | "return"
            ) =>
            {
                return true;
            }
            _ => {}
        }
    }
    true
}

fn rest_hits(file: &File, types: &Types) -> Vec<String> {
    let tests = file.test_tokens();
    let rests = (1..file.tokens.len()).filter(|at| {
        file.tx(*at) == "."
            && file.tx(at + 1) == "."
            && (matches!(file.tx(at + 2), "}" | ")") && file.tx(at - 1) == ","
                || file.tx(at + 2) == "," && matches!(file.tx(at - 1), "," | "("))
            && !tests.get(*at).copied().unwrap_or(false)
    });
    let hit = |at: usize| {
        let open = opening(file, at)?;
        (binds(file, open) && whole(file, open)).then_some(())?;
        let name = own_path(file, types, open)?;
        let line = file.tokens[at].line;
        Some(format!(
            "{}:{line}: `..` after a bound field of `{name}`",
            file.path
        ))
    };
    rests.filter_map(hit).collect()
}

#[test]
fn no_rest_pattern_after_a_bound_field() {
    let files: Vec<File> = support::source_files(&["src"])
        .iter()
        .map(|(relative, path)| File::parse(relative, &support::read(path)))
        .collect();
    let types = types(&files);
    let violations: Vec<String> = files
        .iter()
        .flat_map(|file| rest_hits(file, &types))
        .collect();
    support::report(
        "rest pattern guard: a `let` without `else`, parameter or `for` pattern of a workspace type that binds a field names every field, no `..`.",
        &violations,
    );
}

const SAMPLE: &str = r"struct Pair { a: u8, b: u8 }
struct Wrap(u8, u8);
enum Mode { A, B(u8, u8), C { x: u8, y: u8 } }
fn f(pair: Pair, mode: Mode, key: Key) {
    let Pair { a, .. } = pair;
    let Pair { .. } = pair;
    if let Mode::C { x, .. } = mode {}
    if let Mode::B(n, ..) = mode {}
    if matches!(mode, Mode::C { .. }) {}
    let Key { code, .. } = key;
    let Wrap(first, ..) = wrap;
    if matches!(mode, Mode::C { x: 0, y: Some(_), .. } | Mode::B(Mode::A, ..)) {}
    if let Mode::C { x: ref mut bound @ 1, .. } = mode {}
    if let trash::Mode::C { x, .. } = mode {}
    let Pair { a: first, .. } = pair;
    let Wrap(.., last) = wrap;
    if let Mode::B(first, .., last) = mode {}
    let Mode::C { x, .. } = mode else { return };
    while let Mode::B(n, ..) = mode {}
    if ready && let Some(Pair { a, .. }) = pair {}
    let (n, Pair { a, .. }) = (0, pair);
    for Wrap(first, ..) in wraps {}
    let names = pairs.iter().map(|Pair { a, .. }| a);
    let Pair { a, .. } = if ready { pair } else { other };
    match mode { Mode::B(n, ..) | Mode::C { x: n, .. } => {} Mode::A => {} }
    match SAMPLE { Mode::C { x, .. } => { let Pair { a, .. } = pair; } _ => {} }
}
fn g(Pair { a, .. }: Pair, Wrap(first, ..): Wrap) {}
impl Mode {
    fn k(&self) -> u8 {
        match self {
            Self::C { x, .. } => *x,
            _ => 0,
        }
    }
    fn g(&self) -> Self {
        match Self::A { Self::B(n, ..) => {} _ => {} }
        Self::A
    }
}
#[cfg(test)]
mod tests {
    fn g(pair: Pair) {
        let Pair { a, .. } = pair;
    }
}
";

#[test]
fn rest_pattern_is_seen_only_after_a_binding_outside_tests() {
    let file = File::parse("kernel/src/sample.rs", SAMPLE);
    let types = types(std::slice::from_ref(&file));
    let expected = [
        "kernel/src/sample.rs:5: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:11: `..` after a bound field of `Wrap`",
        "kernel/src/sample.rs:15: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:16: `..` after a bound field of `Wrap`",
        "kernel/src/sample.rs:21: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:22: `..` after a bound field of `Wrap`",
        "kernel/src/sample.rs:23: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:24: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:26: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:28: `..` after a bound field of `Pair`",
        "kernel/src/sample.rs:28: `..` after a bound field of `Wrap`",
    ];
    assert_eq!(rest_hits(&file, &types), expected);
}
