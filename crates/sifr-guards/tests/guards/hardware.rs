// GUARD: a test touching real hardware must carry `#[ignore = "hardware..."]`.

use crate::guards::support::{self, Allow};

const MARKERS: &[&str] = &[
    "spawn_audio",
    "spawn_media",
    "open_stream",
    "recommended_watcher",
    "osascript",
    "run_audio_loop",
];

const ALLOWLIST: &[Allow] = &[];

fn preceding_attributes<'a>(lines: &[&'a str], fn_line_index: usize) -> Vec<&'a str> {
    let mut attributes = Vec::new();
    let mut index = fn_line_index;
    while index > 0 {
        index -= 1;
        let Some(trimmed) = lines.get(index).map(|line| line.trim_start()) else {
            break;
        };
        if trimmed.starts_with("#[") {
            attributes.push(trimmed);
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        break;
    }
    attributes
}

fn is_ignored_for_hardware(attributes: &[&str]) -> bool {
    attributes
        .iter()
        .any(|attribute| attribute.contains("ignore") && attribute.contains("hardware"))
}

fn is_a_test(attributes: &[&str]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.contains("#[test]") || attribute.contains("#[rstest]")
    })
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn names_marker(block: &str, marker: &str) -> bool {
    let bytes = block.as_bytes();
    let mut start = 0;
    while let Some(offset) = block.get(start..).and_then(|slice| slice.find(marker)) {
        let index = start + offset;
        let before_ok = index
            .checked_sub(1)
            .and_then(|position| bytes.get(position))
            .is_none_or(|byte| !is_word_byte(*byte));
        let after = index + marker.len();
        let after_ok = bytes.get(after).is_none_or(|byte| !is_word_byte(*byte));
        if before_ok && after_ok {
            return true;
        }
        start = index + 1;
    }
    false
}

#[test]
fn every_hardware_touching_test_body_is_ignored() {
    let files = support::source_files(&["src", "tests"]);
    assert!(!files.is_empty(), "expected to find crate sources");

    let mut violations: Vec<String> = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    for (relative, path) in &files {
        let content = support::read(path);
        let lines: Vec<&str> = content.lines().collect();
        let declarations = support::parameters(&content);

        for (position, (line_number, name, _parameters)) in
            declarations.iter().enumerate()
        {
            let start = line_number - 1;
            let end = declarations
                .get(position + 1)
                .map_or(lines.len(), |(next_line, ..)| next_line - 1);
            let Some(body) = lines.get(start..end) else {
                continue;
            };
            let attributes = preceding_attributes(&lines, start);
            if !is_a_test(&attributes) {
                continue;
            }

            let block = body.join("\n");
            let Some(marker) =
                MARKERS.iter().find(|marker| names_marker(&block, marker))
            else {
                continue;
            };

            if is_ignored_for_hardware(&attributes) {
                continue;
            }

            for row in ALLOWLIST {
                if row.path == *relative && block.contains(row.pattern) {
                    seen.push((row.path.to_owned(), row.pattern));
                }
            }
            let excused = ALLOWLIST
                .iter()
                .any(|row| row.path == *relative && block.contains(row.pattern));
            if !excused {
                violations.push(format!(
                    "{relative}:{line_number}: `{name}` calls `{marker}` without \
                     `#[ignore = \"hardware...\"]`"
                ));
            }
        }
    }

    let stale: Vec<String> = ALLOWLIST
        .iter()
        .filter(|row| {
            !seen
                .iter()
                .any(|(path, pattern)| path == row.path && *pattern == row.pattern)
        })
        .map(|row| format!("{}: `{}` ({})", row.path, row.pattern, row.reason))
        .collect();

    support::report(
        "hardware guard: a test naming one of the driver-spawning or shell-out \
         functions listed in MARKERS must carry #[ignore = \"hardware...\"].",
        &violations,
        &stale,
    );
}
