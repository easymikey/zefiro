// GUARD: a comment is a violation unless it is a listed one-line survivor.

use std::collections::BTreeSet;

use crate::guards::{support, support::Allow};

const ATTRIBUTE_RULES: &[support::Rule] = &[
    support::Rule {
        pattern: concat!("#[", "doc", " ="),
        message: "a doc attribute is a comment",
    },
    support::Rule {
        pattern: concat!("#![", "doc", " ="),
        message: "a doc attribute is a comment",
    },
    support::Rule {
        pattern: concat!("#[", "expect", "("),
        message: "an expect attribute silences a lint — fix the code instead",
    },
    support::Rule {
        pattern: concat!("#![", "expect", "("),
        message: "an expect attribute silences a lint — fix the code instead",
    },
];

const ALLOW: &[Allow] = &[
    Allow::new(
        "sifr-guards/tests/guards/comments.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/config_doc_appearance.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/conventions.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/lexer.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/demeter.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/demeter_views.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/dispatch.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/errors.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/forbidden_names.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/hardware.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/imports.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/layering.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/macros.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/mod.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/purity.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/support.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/fault.rs",
        "GUARD:",
        "one line saying why this shared module exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/length.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
    Allow::new(
        "sifr-guards/tests/guards/naming.rs",
        "GUARD:",
        "one line saying why this guard exists",
    ),
];

enum State {
    Code,
    Text,
    Raw(usize),
}

struct Comment {
    line: usize,
    leading: bool,
    token: String,
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn raw_hashes(bytes: &[u8], from: usize) -> Option<usize> {
    let mut hashes = 0;
    while bytes.get(from + hashes) == Some(&b'#') {
        hashes += 1;
    }
    if bytes.get(from + hashes) == Some(&b'"') {
        Some(hashes)
    } else {
        None
    }
}

fn closes_raw(bytes: &[u8], from: usize, hashes: usize) -> bool {
    (0..hashes).all(|offset| bytes.get(from + offset) == Some(&b'#'))
}

fn token_of(body: &str) -> String {
    body.trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned()
}

fn comments(content: &str) -> Vec<Comment> {
    let bytes = content.as_bytes();
    let mut out: Vec<Comment> = Vec::new();
    let mut state = State::Code;
    let mut index = 0;
    let mut line = 1;
    let mut line_start = 0;
    while let Some(&byte) = bytes.get(index) {
        match state {
            State::Code => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    let rest = content.get(index..).unwrap_or("");
                    let end = rest.find('\n').unwrap_or(rest.len());
                    let body = rest.get(..end).unwrap_or("");
                    let prefix = content.get(line_start..index).unwrap_or("");
                    out.push(Comment {
                        line,
                        leading: prefix.trim().is_empty(),
                        token: token_of(body),
                    });
                    index += end;
                } else if byte == b'"' {
                    state = State::Text;
                    index += 1;
                } else if byte == b'\'' {
                    index += char_literal_width(bytes, index);
                } else if byte == b'r'
                    && !bytes
                        .get(index.wrapping_sub(1))
                        .copied()
                        .is_some_and(is_word_byte)
                    && let Some(hashes) = raw_hashes(bytes, index + 1)
                {
                    state = State::Raw(hashes);
                    index += 1 + hashes + 1;
                } else {
                    if byte == b'\n' {
                        line += 1;
                        line_start = index + 1;
                    }
                    index += 1;
                }
            }
            State::Text => {
                if byte == b'\\' {
                    index += 2;
                } else {
                    if byte == b'"' {
                        state = State::Code;
                    } else if byte == b'\n' {
                        line += 1;
                        line_start = index + 1;
                    }
                    index += 1;
                }
            }
            State::Raw(hashes) => {
                if byte == b'"' && closes_raw(bytes, index + 1, hashes) {
                    state = State::Code;
                    index += 1 + hashes;
                } else {
                    if byte == b'\n' {
                        line += 1;
                        line_start = index + 1;
                    }
                    index += 1;
                }
            }
        }
    }
    out
}

fn char_literal_width(bytes: &[u8], index: usize) -> usize {
    if bytes.get(index + 1) == Some(&b'\\') {
        let mut width = 2;
        while let Some(&byte) = bytes.get(index + width) {
            width += 1;
            if byte == b'\'' {
                break;
            }
        }
        return width;
    }
    if bytes.get(index + 2) == Some(&b'\'') {
        return 3;
    }
    1
}

fn allow_row(path: &str, token: &str) -> Option<&'static Allow> {
    ALLOW
        .iter()
        .find(|row| row.path == path && row.pattern == token)
}

#[test]
fn every_comment_is_a_listed_one_liner() {
    let mut violations: Vec<String> = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    let files = support::source_files(&["src", "tests", "benches", "examples"]);
    assert!(!files.is_empty(), "expected to find workspace sources");

    for (relative, path) in files {
        let content = support::read(&path);
        let found = comments(&content);
        let lines: BTreeSet<usize> = found.iter().map(|comment| comment.line).collect();
        for comment in &found {
            let number = comment.line;
            if !comment.leading {
                violations.push(format!(
                    "{relative}:{number}: a comment after code — the name says it, or docs/ does"
                ));
            } else if comment.token == "SAFETY:" {
                if lines.contains(&(number + 1)) {
                    violations.push(format!(
                        "{relative}:{number}: `SAFETY:` runs past one line"
                    ));
                }
            } else if let Some(row) = allow_row(&relative, &comment.token) {
                if lines.contains(&(number + 1)) {
                    violations.push(format!(
                        "{relative}:{number}: `{}` runs past one line",
                        row.pattern
                    ));
                } else {
                    seen.push((relative.clone(), row.pattern));
                }
            } else {
                violations.push(format!(
                    "{relative}:{number}: a comment — delete it, or list it in the comments guard"
                ));
            }
        }
        for (index, raw_line) in content.lines().enumerate() {
            if let Some(rule) = support::rule_hit(raw_line, ATTRIBUTE_RULES) {
                violations.push(format!("{relative}:{}: {}", index + 1, rule.message));
            }
        }
    }

    support::report_with_stale(
        "comments guard (docs/principles.md): the code has no comments. The only \
         survivors are a one-line `SAFETY:` above an unsafe block and the \
         one-liners listed in this guard's ALLOW — a `PROTOCOL:` recording a terminal or AppKit event, \
         a `GUARD:` saying why a guard exists — each exactly one line.",
        &violations,
        &support::stale(ALLOW, &seen),
    );
}
