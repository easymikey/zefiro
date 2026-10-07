use std::collections::BTreeSet;

use crate::guards::{
    lexer::{File, is_word},
    support,
};

type Enums = BTreeSet<(String, String)>;

fn crate_of(path: &str) -> &str {
    path.split('/').next().unwrap_or("")
}

fn bracket(text: &str) -> i32 {
    match text {
        "(" | "[" | "{" => 1,
        ")" | "]" | "}" => -1,
        _ => 0,
    }
}

fn enums(files: &[File]) -> Enums {
    let pairs = files.iter().flat_map(|file| {
        let owner = crate_of(&file.path).to_owned();
        let named = file.items.iter().filter(|site| site.kind == "enum");
        named.map(move |site| (owner.clone(), site.name.clone()))
    });
    pairs.collect()
}

fn body_open(file: &File, at: usize) -> Option<usize> {
    let mut depth = 0_i32;
    (at + 1..file.tokens.len()).find(|x| {
        let text = file.tx(*x);
        let found = depth == 0 && text == "{";
        depth += bracket(text);
        found
    })
}

fn arrow_after(file: &File, from: usize, close: usize) -> Option<usize> {
    let mut depth = 0_i32;
    (from..close).find(|at| {
        depth += bracket(file.tx(*at));
        depth == 0 && file.tx(*at) == "=" && file.tx(at + 1) == ">"
    })
}

fn arm_end(file: &File, arrow: usize, close: usize) -> usize {
    let body = arrow + 2;
    let block_end = file.matching_close(body) + 1;
    if file.tx(body) == "{" && !matches!(file.tx(block_end), "." | "?") {
        return block_end + usize::from(file.tx(block_end) == ",");
    }
    let mut depth = 0_i32;
    let end = (body..close).find(|at| {
        depth += bracket(file.tx(*at));
        depth == 0 && file.tx(*at) == ","
    });
    end.map_or(close, |at| at + 1)
}

fn patterns(file: &File, open: usize) -> Vec<(usize, usize)> {
    let close = file.matching_close(open);
    let (mut out, mut at) = (Vec::new(), open + 1);
    while let Some(arrow) = arrow_after(file, at, close) {
        out.push((at, arrow));
        at = arm_end(file, arrow, close);
    }
    out
}

fn path_at(file: &File, from: usize) -> Vec<&str> {
    let mut out = vec![file.tx(from)];
    let mut at = from;
    while file.tx(at + 1) == ":" && file.tx(at + 2) == ":" {
        at += 3;
        out.push(file.tx(at));
    }
    out
}

fn impl_header(file: &File, at: usize) -> Option<(usize, &str)> {
    let (mut depth, mut target) = (0_i32, "");
    for x in at + 1..file.tokens.len() {
        match file.tx(x) {
            "{" | "where" => {
                let open = (x..file.tokens.len()).find(|y| file.tx(*y) == "{")?;
                return Some((open, target));
            }
            ";" | "" => return None,
            "<" => depth += 1,
            ">" if file.tx(x - 1) != "-" => depth -= 1,
            word if depth == 0 && is_word(word) => target = word,
            _ => {}
        }
    }
    None
}

fn impl_target(file: &File, at: usize) -> &str {
    let impls = (0..at).filter(|x| file.tx(*x) == "impl");
    let inside = impls
        .filter_map(|x| impl_header(file, x))
        .filter(|(open, _)| *open < at && at < file.matching_close(*open));
    inside
        .max_by_key(|(open, _)| *open)
        .map_or("", |(_, name)| name)
}

fn own_enum(file: &File, enums: &Enums, from: usize) -> Option<String> {
    let path = path_at(file, from);
    let [prefix @ .., name, _variant] = path.as_slice() else {
        return None;
    };
    let name = if *name == "Self" {
        impl_target(file, from)
    } else {
        name
    };
    let owner = match prefix.first() {
        None | Some(&"crate") | Some(&"self") => crate_of(&file.path),
        Some(first) => first,
    };
    let key = (owner.to_owned(), name.to_owned());
    enums.contains(&key).then(|| name.to_owned())
}

fn only_wildcards(file: &File, (from, to): (usize, usize)) -> bool {
    to > from + 3
        && file.tx(from) == "("
        && file.matching_close(from) + 1 == to
        && (from + 1..to - 1).all(|at| matches!(file.tx(at), "_" | ","))
}

fn alternatives(file: &File, (from, to): (usize, usize)) -> Vec<(usize, usize)> {
    let mut depth = 0_i32;
    let end = (from..to)
        .find(|at| {
            depth += bracket(file.tx(*at));
            depth == 0 && file.tx(*at) == "if"
        })
        .unwrap_or(to);
    let mut depth = 0_i32;
    let bars: Vec<usize> = (from..end)
        .filter(|at| {
            depth += bracket(file.tx(*at));
            depth == 0 && file.tx(*at) == "|"
        })
        .collect();
    let starts = std::iter::once(from).chain(bars.iter().map(|bar| bar + 1));
    let ends = bars.iter().copied().chain(std::iter::once(end));
    starts
        .zip(ends)
        .filter(|(start, stop)| stop > start)
        .collect()
}

fn tuple_wildcard_hit(file: &File, enums: &Enums, open: usize) -> Option<String> {
    let arms = patterns(file, open);
    let (wildcard, shape) = arms.iter().find_map(|arm| {
        let alternatives = alternatives(file, *arm);
        let is_alternation = alternatives.len() > 1;
        alternatives.into_iter().find_map(|(from, to)| {
            if only_wildcards(file, (from, to)) {
                Some((from, "(_, _)"))
            } else if is_alternation && to == from + 1 && file.tx(from) == "_" {
                Some((from, "_"))
            } else {
                None
            }
        })
    })?;
    let name = arms
        .iter()
        .find_map(|(from, to)| (*from..*to).find_map(|at| own_enum(file, enums, at)))?;
    let line = file.tokens[wildcard].line;
    Some(format!("{}:{line}: `{shape} =>` on `{name}`", file.path))
}

fn wildcard_hits(file: &File, enums: &Enums) -> Vec<String> {
    let tests = file.test_tokens();
    let opens: Vec<usize> = (0..file.tokens.len())
        .filter(|at| file.tx(*at) == "match")
        .filter_map(|at| body_open(file, at))
        .collect();
    let hit = |open: &usize| {
        let arms = patterns(file, *open);
        let wildcard = arms
            .iter()
            .find(|(from, to)| *to == from + 1 && file.tx(*from) == "_")?;
        let name = arms
            .iter()
            .find_map(|(from, _)| own_enum(file, enums, *from))?;
        let line = file.tokens[wildcard.0].line;
        Some(format!("{}:{line}: `_ =>` on `{name}`", file.path))
    };
    let tuples = opens
        .iter()
        .filter(|open| !tests.get(**open).copied().unwrap_or(false))
        .filter_map(|open| tuple_wildcard_hit(file, enums, *open));
    opens.iter().filter_map(hit).chain(tuples).collect()
}

#[test]
fn no_wildcard_arm_over_an_own_enum() {
    let files: Vec<File> = support::source_files(&["src", "tests"])
        .iter()
        .map(|(relative, path)| File::parse(relative, &support::read(path)))
        .collect();
    let enums = enums(&files);
    let violations: Vec<String> = files
        .iter()
        .flat_map(|file| wildcard_hits(file, &enums))
        .collect();
    support::report(
        "wildcard arm guard: name every variant of an enum of the same crate, no `_ =>`.",
        &violations,
        &[],
    );
}

const SAMPLE: &str = r"enum Mode { A, B(u8), C { x: u8 } }
fn f(mode: Mode, key: Key) {
    match mode {
        Mode::A => {}
        Mode::B(n) if n > 0 => g(n),
        _ => {}
    }
    match key { Key::Up => {} _ => {} }
    match mode { crate::Mode::A | Mode::C { .. } => h(), other => {} }
}
impl Mode {
    fn k(&self) -> u8 {
        match self {
            Self::A => 1,
            _ => 0,
        }
    }
}
fn pair(left: Mode, right: Mode, key: Key) {
    match (left, right) {
        (Mode::A, Mode::A) => {}
        (_, _) => {}
    }
    match (key, key) { (Key::Up, _) => {} (_, _) => {} }
    match (left, key) { (Mode::A, _) => {} (_, Key::Up) | (_, _) => {} }
    match left { Mode::B(_) | _ => {} }
}
#[cfg(test)]
mod tests {
    fn g(left: Mode, right: Mode) {
        match (left, right) { (Mode::A, _) => {} (_, _) => {} }
    }
}
";

#[test]
fn wildcard_arm_is_seen_only_over_an_own_enum() {
    let file = File::parse("kernel/src/sample.rs", SAMPLE);
    let enums = enums(std::slice::from_ref(&file));
    let expected = [
        "kernel/src/sample.rs:6: `_ =>` on `Mode`",
        "kernel/src/sample.rs:15: `_ =>` on `Mode`",
        "kernel/src/sample.rs:22: `(_, _) =>` on `Mode`",
        "kernel/src/sample.rs:25: `(_, _) =>` on `Mode`",
        "kernel/src/sample.rs:26: `_ =>` on `Mode`",
    ];
    assert_eq!(wildcard_hits(&file, &enums), expected);
}
