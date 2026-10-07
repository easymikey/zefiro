use crate::guards::{lexer::File, support};

const UNITS: [&str; 3] = ["from_mins", "from_hours", "from_days"];

fn is_fixed(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_digit())
        || (word.contains(|c: char| c.is_ascii_uppercase())
            && word
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

fn is_fixed_argument(file: &File, open: usize) -> bool {
    let close = file.matching_close(open);
    let words: Vec<&str> = (open + 1..close).map(|at| file.tx(at)).collect();
    let is_path = words.iter().all(|word| {
        *word == ":" || word.chars().all(|c| c.is_alphanumeric() || c == '_')
    });
    is_path && words.last().is_some_and(|word| is_fixed(word))
}

fn duration_hits(file: &File) -> Vec<String> {
    let tests = file.test_tokens();
    (0..file.tokens.len())
        .filter(|at| {
            file.tx(*at) == "Duration"
                && file.tx(at + 1) == ":"
                && file.tx(at + 2) == ":"
                && UNITS.contains(&file.tx(at + 3))
                && file.tx(at + 4) == "("
                && !is_fixed_argument(file, at + 4)
                && !tests.get(*at).copied().unwrap_or(false)
        })
        .map(|at| {
            let unit = file.tx(at + 3);
            format!("{}:{}: `Duration::{unit}`", file.path, file.tokens[at].line)
        })
        .collect()
}

#[test]
fn no_duration_unit_over_a_runtime_value() {
    let violations: Vec<String> = support::source_files(&["src"])
        .iter()
        .flat_map(|(relative, path)| {
            duration_hits(&File::parse(relative, &support::read(path)))
        })
        .collect();
    support::report(
        "duration guard: `Duration::from_mins/from_hours/from_days` panic on overflow; take a literal or a const, else `from_secs` with a saturating product.",
        &violations,
        &[],
    );
}

const SAMPLE: &str = r"const LONG: u64 = 9;
fn presets(minutes: u64) {
    let a = Duration::from_mins(15);
    let b = Duration::from_hours(LONG);
    let c = Duration::from_days(Self::LONG);
    let d = Duration::from_mins(minutes);
    let e = Duration::from_hours(minutes * 2);
    let f = Duration::from_days(LONG + 1);
}
#[cfg(test)]
mod tests {
    fn g(minutes: u64) {
        let a = Duration::from_mins(minutes);
    }
}
";

#[test]
fn duration_unit_is_seen_only_over_a_runtime_value_outside_tests() {
    let file = File::parse("kernel/src/sample.rs", SAMPLE);
    let expected = [
        "kernel/src/sample.rs:6: `Duration::from_mins`",
        "kernel/src/sample.rs:7: `Duration::from_hours`",
        "kernel/src/sample.rs:8: `Duration::from_days`",
    ];
    assert_eq!(duration_hits(&file), expected);
}
