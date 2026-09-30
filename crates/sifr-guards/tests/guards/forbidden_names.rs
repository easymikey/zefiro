// GUARD: the Forbidden list from the naming rules, gated per crate so kernel is
// asserted now.

use crate::guards::support;

fn crate_of(relative: &str) -> &str {
    relative.split('/').next().unwrap_or(relative)
}

fn gate_by_crate(
    rule: &str,
    violations: &[String],
    pending: &[&'static str],
) -> (Vec<String>, Vec<String>) {
    let mut hard = Vec::new();
    let mut pending_hit: Vec<&str> = Vec::new();
    for violation in violations {
        let crate_name = crate_of(violation);
        if pending.contains(&crate_name) {
            if !pending_hit.contains(&crate_name) {
                pending_hit.push(crate_name);
            }
        } else {
            hard.push(violation.clone());
        }
    }
    let stale = pending
        .iter()
        .filter(|name| !pending_hit.contains(name))
        .map(|name| {
            format!(
                "{rule}: `{name}` is listed pending but has no violations — remove it"
            )
        })
        .collect();
    (hard, stale)
}

const VERB_MODULE_FILES: &[&str] = &[
    "reduce.rs",
    "compile.rs",
    "route.rs",
    "handle.rs",
    "process.rs",
    "dispatch.rs",
];
const VERB_MODULE_PENDING: &[&str] = &[];

#[test]
fn no_verb_module_file_names() {
    let mut violations = Vec::new();
    for (relative, _) in support::source_files(&["src"]) {
        let Some(name) = relative.rsplit('/').next() else {
            continue;
        };
        if VERB_MODULE_FILES.contains(&name) {
            violations.push(format!("{relative}: verb module file name `{name}`"));
        }
    }
    let (hard, stale) =
        gate_by_crate("verb module names", &violations, VERB_MODULE_PENDING);
    support::report(
        "naming guard: a module file names what it holds, not the verb it performs.",
        &hard,
        &stale,
    );
}

const GET_PREFIX_PENDING: &[&str] = &[];

#[test]
fn no_get_prefix_functions() {
    let mut violations = Vec::new();
    for (relative, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for (line, name, _) in support::parameters(&content) {
            if name.starts_with("get_") {
                violations
                    .push(format!("{relative}:{line}: `fn {name}` — get_ prefix"));
            }
        }
    }
    let (hard, stale) = gate_by_crate("get_ prefixes", &violations, GET_PREFIX_PENDING);
    support::report(
        "naming guard: a getter is the field name, never get_*.",
        &hard,
        &stale,
    );
}

const PREDICATE_PENDING: &[&str] = &[];

fn is_forbidden_predicate(name: &str) -> bool {
    name.starts_with("should_")
        || name.starts_with("wants_")
        || name.starts_with("needs_")
}

#[test]
fn no_should_wants_needs_predicates() {
    let mut violations = Vec::new();
    for (relative, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for (line, name, _) in support::parameters(&content) {
            if is_forbidden_predicate(&name) {
                violations.push(format!(
                    "{relative}:{line}: `fn {name}` — should_/wants_/needs_ predicate"
                ));
            }
        }
    }
    let (hard, stale) = gate_by_crate(
        "should_/wants_/needs_ predicates",
        &violations,
        PREDICATE_PENDING,
    );
    support::report(
        "naming guard: a predicate is is_*/has_*/can_*, never should_/wants_/needs_.",
        &hard,
        &stale,
    );
}

const SELF_RETURNING_PREFIXES: &[&str] = &[
    "compile", "build_", "make_", "create_", "place_", "resolve_",
];
const SELF_RETURNING_PENDING: &[&str] = &[];

fn returns_self(content: &str, after: usize) -> bool {
    let window_end = (after + 400).min(content.len());
    let Some(window) = content.get(after..window_end) else {
        return false;
    };
    let stop = window.find(['{', ';']).unwrap_or(window.len());
    let Some(tail) = window.get(..stop) else {
        return false;
    };
    tail.split("->").nth(1).is_some_and(|after_arrow| {
        let word: String = after_arrow
            .trim()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        word == "Self"
    })
}

fn is_self_returning_name(name: &str) -> bool {
    name == "compile"
        || SELF_RETURNING_PREFIXES
            .iter()
            .any(|prefix| *prefix != "compile" && name.starts_with(prefix))
}

fn line_start_offset(content: &str, line: usize) -> usize {
    content
        .lines()
        .take(line.saturating_sub(1))
        .map(|preceding| preceding.len() + 1)
        .sum()
}

#[test]
fn no_constructors_named_by_mechanism() {
    let mut violations = Vec::new();
    for (relative, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for (line, name, _) in support::parameters(&content) {
            if !is_self_returning_name(&name) {
                continue;
            }
            let offset = line_start_offset(&content, line);
            if returns_self(&content, offset) {
                violations.push(format!(
                    "{relative}:{line}: `fn {name}` returns Self — name it new/with_*/from_*"
                ));
            }
        }
    }
    let (hard, stale) = gate_by_crate(
        "constructors named by mechanism",
        &violations,
        SELF_RETURNING_PENDING,
    );
    support::report(
        "naming guard: compile/build_*/make_*/create_*/place_*/resolve_* never return Self.",
        &hard,
        &stale,
    );
}

const FORBIDDEN_TYPE_SUFFIXES: &[&str] = &["Spec", "Slot"];
const TYPE_DECL_KEYWORDS: &[&str] = &["struct", "enum", "type", "trait"];
const TYPE_SUFFIX_CRATE_PENDING: &[&str] = &["widgets"];
const TYPE_SUFFIX_NAME_PENDING: &[(&str, &str)] =
    &[("kernel/src/domain/setting_row.rs", "SettingEntry")];

fn is_pending_type(relative: &str, name: &str) -> bool {
    TYPE_SUFFIX_NAME_PENDING
        .iter()
        .any(|(path, pending_name)| *path == relative && *pending_name == name)
}

fn declared_type_names(content: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for raw_line in content.lines() {
        let words: Vec<&str> = raw_line.split_whitespace().collect();
        for (position, word) in words.iter().enumerate() {
            if !TYPE_DECL_KEYWORDS.contains(word) {
                continue;
            }
            let Some(name) = words.get(position + 1) else {
                continue;
            };
            let name = name.trim_end_matches(['<', '{', '(', ';', ':']);
            if FORBIDDEN_TYPE_SUFFIXES
                .iter()
                .any(|suffix| name.ends_with(suffix))
            {
                names.push(name);
            }
        }
    }
    names
}

#[test]
fn no_forbidden_type_suffixes() {
    let mut violations = Vec::new();
    let mut name_pending_hit: Vec<(String, String)> = Vec::new();
    for (relative, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for name in declared_type_names(&content) {
            if is_pending_type(&relative, name) {
                name_pending_hit.push((relative.clone(), name.to_owned()));
                continue;
            }
            violations.push(format!("{relative}: `{name}` — Spec/Slot type suffix"));
        }
    }
    let (hard, crate_stale) =
        gate_by_crate("Spec/Slot suffixes", &violations, TYPE_SUFFIX_CRATE_PENDING);
    let name_stale: Vec<String> = TYPE_SUFFIX_NAME_PENDING
        .iter()
        .filter(|(path, name)| {
            !name_pending_hit
                .iter()
                .any(|(seen_path, seen_name)| seen_path == path && seen_name == name)
        })
        .map(|(path, name)| {
            format!("Spec/Slot suffixes: `{path}:{name}` is pending but has no hit — remove it")
        })
        .collect();
    support::report(
        "naming guard: no Spec/Slot type suffix — Kind/Info/Data/Manager/Helper are the \
         existing mechanism-suffix guard's job.",
        &hard,
        &[crate_stale, name_stale].concat(),
    );
}

const REFUSED_FAULT_PROBLEM_PENDING: &[&str] = &["widgets", "audio", "runtime"];

fn ends_in_third_word(word: &str) -> bool {
    (word.len() > "Refused".len() && word.ends_with("Refused"))
        || (word.len() > "Fault".len() && word.ends_with("Fault"))
        || (word.len() > "Problem".len() && word.ends_with("Problem"))
}

#[test]
fn no_refused_fault_problem_identifiers() {
    let mut violations = Vec::new();
    for (relative, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for (index, raw_line) in content.lines().enumerate() {
            for word in raw_line.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                if ends_in_third_word(word) {
                    violations.push(format!(
                        "{relative}:{}: `{word}` — Refused/Fault/Problem",
                        index + 1
                    ));
                }
            }
        }
    }
    let (hard, stale) = gate_by_crate(
        "Refused/Fault/Problem identifiers",
        &violations,
        REFUSED_FAULT_PROBLEM_PENDING,
    );
    support::report(
        "naming guard: an error's third word is never Refused/Fault/Problem.",
        &hard,
        &stale,
    );
}
