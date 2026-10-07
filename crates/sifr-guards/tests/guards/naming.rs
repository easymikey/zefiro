// GUARD: our own identifiers are full words, never project-made abbreviations.

use std::{fs, path::Path};

use crate::guards::support;

fn strip_comments_and_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        out.push(c);
    }
    out
}

fn tokens(line: &str) -> Vec<&str> {
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, byte) in line.bytes().enumerate() {
        match (is_word(byte), start) {
            (true, None) => start = Some(i),
            (false, Some(begin)) => {
                out.extend(line.get(begin..i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        out.extend(line.get(begin..));
    }
    out
}

enum Match {
    Exact(&'static str),
    Prefix(&'static str),
    Suffix(&'static str),
}

type AbbreviationRule = (Match, &'static [&'static str], &'static str);

const ABBREVIATION_RULES: &[AbbreviationRule] = &[
    (Match::Exact("vol"), &[], "`vol` — spell out `volume`"),
    (Match::Exact("proto"), &[], "`proto` — spell out `protocol`"),
    (
        Match::Exact("cm"),
        &[],
        "`cm` — spell out `metrics`/`card_metrics`",
    ),
    (
        Match::Exact("hw"),
        &[],
        "`hw` — spell out what it actually measures",
    ),
    (Match::Exact("dur"), &[], "`dur` — spell out `duration`"),
    (Match::Exact("tech"), &[], "`tech` — spell out `format`"),
    (
        Match::Suffix("_vol"),
        &[],
        "ends in `_vol` — spell out `volume`",
    ),
    (
        Match::Prefix("vol_"),
        &[],
        "starts with `vol_` — spell out `volume_`",
    ),
    (
        Match::Prefix("eq_"),
        &["eq_ignore_ascii_case"],
        "starts with `eq_` — spell out `spectrum_`",
    ),
    (
        Match::Prefix("tech_"),
        &[],
        "starts with `tech_` — spell out `format_`",
    ),
    (
        Match::Prefix("draw"),
        &["draw", "draw_pixmap"],
        "`draw` — say `paint` (into a buffer) or `render` (build a widget)",
    ),
    (
        Match::Suffix("_draw"),
        &[],
        "`draw` — say `paint` (into a buffer) or `render` (build a widget)",
    ),
    (
        Match::Suffix("_proto"),
        &[],
        "ends in `_proto` — spell out `_protocol`",
    ),
    (
        Match::Suffix("_dur"),
        &[],
        "ends in `_dur` — spell out `_duration`",
    ),
];

fn denylist_reason(token: &str) -> Option<&'static str> {
    ABBREVIATION_RULES
        .iter()
        .find(|(matches, exceptions, _)| {
            let hit = match matches {
                Match::Exact(pattern) => token == *pattern,
                Match::Prefix(pattern) => token.starts_with(pattern),
                Match::Suffix(pattern) => token.ends_with(pattern),
            };
            hit && !exceptions.contains(&token)
        })
        .map(|(_, _, message)| *message)
}

const DENIED_PARAMETERS: &[&str] = &[
    "data", "info", "ctx", "cfg", "opts", "options", "idx", "tmp", "res", "val",
    "value", "handle", "item", "entry", "thing", "stuff", "params", "args", "props",
    "w", "h", "n", "i",
];

const GEOMETRY_FILES: &[&str] = &["widgets/src/geometry.rs", "widgets/src/node.rs"];

const GEOMETRY_PARAMETERS: &[&str] = &["x", "y", "w", "h", "value"];

fn parameter_name(slot: &str) -> Option<String> {
    let bytes = slot.as_bytes();
    let mut i = 0;
    while let Some(byte) = bytes.get(i) {
        if *byte == b':' {
            if bytes.get(i + 1) == Some(&b':') {
                i += 2;
                continue;
            }
            let before = slot.get(..i)?;
            return tokens(before).last().map(|name| (*name).to_owned());
        }
        i += 1;
    }
    None
}

const MECHANISM_TYPE_SUFFIXES: &[&str] = &[
    "Props", "Tuning", "Sync", "Scratch", "Slices", "Info", "Type", "Kind", "Inputs",
    "Values", "Flags", "Params", "Options", "Data", "Manager", "Handler", "Helper",
    "Util", "Utils", "Wrapper", "Holder", "Draw",
];

const TYPE_DECL_KEYWORDS: &[&str] = &["struct", "enum", "type", "trait"];

const RETIRED_NAMES: &[&str] = &[
    "CoverPlan",
    "PixelCoverStyle",
    "CoverAction",
    "CoverPaint",
    "CoverDraw",
    "FramePrepaint",
    "DeferredDraw",
    "CoverPixels",
    "Skin",
    "ActiveSkin",
    "Keys",
    "KeysConfig",
    "KeyGroup",
    "Group",
    "PaneChrome",
    "ModalFrame",
    "DrawnRows",
    "Watching",
    "Watched",
    "Ui",
    "UiRequest",
    "UiPreset",
    "UiMessage",
    "PlaybackMessage",
    "QueueMessage",
    "LoadedMessage",
    "SettingsRowMessage",
    "TextMessage",
    "JumpMessage",
    "OverlayKind",
    "OverlayScreen",
    "SettingKind",
    "SaveBannerKind",
    "SaveBanner",
    "SaveOutcome",
    "PlaylistSlot",
    "ViewRow",
    "AudioFault",
    "FrameInputs",
    "FrameRenderInputs",
    "WarpParams",
    "InjectParams",
    "SettingsValues",
    "VolumeMode",
    "VolumeConfig",
    "BatchFlags",
    "MediaWorker",
    "RingBuf",
    "NoticeLevel",
    "NoticeLifetime",
    "NoticeUpdate",
    "NoticeOnScreen",
    "ToastUpdate",
    "NoticeDurations",
    "Palette",
    "BarRoles",
    "BarPalette",
    "PaneAreas",
    "PaneColors",
    "PaneFrame",
    "PanePlacement",
    "PaneTitleColors",
    "PaneMetrics",
    "SettingsReadout",
    "EffectStage",
    "EffectTimings",
    "MediaResult",
    "MediaEvent",
    "MediaWatch",
    "WatchHandle",
    "WatchedRefused",
    "WindowTint",
    "Footer",
    "FooterContent",
    "SettingsHintLabels",
    "TechChips",
    "TechChipColors",
    "RendererMode",
    "KeysError",
    "KeyEntry",
    "BindingAction",
    "KeyEffect",
    "Focus",
    "FocusRow",
    "HistoryLine",
    "Adjusted",
    "ResizeSleepCursor",
    "HotReloadPoll",
    "WatcherPollTiming",
    "ModalColors",
    "ModalChromeColors",
    "MeterKey",
    "MeterPlan",
    "ProgressImageKey",
    "ProgressImageSpec",
    "AppearanceSetting",
    "Arm",
    "BindingSource",
    "ConfirmDelete",
    "Cued",
    "CurrentTrack",
    "DeleteCandidate",
    "Disabled",
    "Enabled",
    "FrameWidth",
    "HardwareWatch",
    "Heard",
    "Notified",
    "Other",
    "PlacedSize",
    "Preload",
    "ProgressLine",
    "ProgressParts",
    "ProgressRemaining",
    "Publish",
    "QueryAnswer",
    "QueuePosition",
    "RawModeDisabled",
    "Requested",
    "ScanProgress",
    "TrackDeleted",
    "TrackRef",
];

const RETIRED_DECL_KEYWORDS: &[&str] = &["struct", "enum", "type", "trait", "mod"];

fn type_mechanism_reason(name: &str) -> Option<&'static str> {
    for suffix in MECHANISM_TYPE_SUFFIXES {
        if name.ends_with(suffix) {
            return Some("mechanism-word type-name suffix");
        }
    }
    if name.contains("Cfg") || name.contains("Ctx") {
        return Some("`Cfg`/`Ctx` abbreviation in type name");
    }
    None
}

fn sync_module_directories() -> Vec<String> {
    let crates_dir = support::crates_dir();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&crates_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        sync_directories(&entry.path().join("src"), &crates_dir, &mut out);
    }
    out.sort();
    out
}

fn sync_directories(dir: &Path, crates_dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "sync")
            && let Ok(relative) = path.strip_prefix(crates_dir)
        {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
        sync_directories(&path, crates_dir, out);
    }
}

#[test]
fn no_mechanism_names() {
    let mut violations: Vec<String> = Vec::new();

    let flag = |rel: &str, name: &str, reason: &str, violations: &mut Vec<String>| {
        violations.push(format!("{rel}: {reason} (found `{name}`)"));
    };

    for (rel, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for raw_line in content.lines() {
            let stripped = strip_comments_and_strings(raw_line);
            let toks = tokens(&stripped);
            for (i, token) in toks.iter().enumerate() {
                if TYPE_DECL_KEYWORDS.contains(token) {
                    if let Some(name) = toks.get(i + 1)
                        && let Some(reason) = type_mechanism_reason(name)
                    {
                        flag(&rel, name, reason, &mut violations);
                    }
                } else if *token == "fn"
                    && let Some(name) = toks.get(i + 1)
                    && name.starts_with("sync_")
                {
                    flag(
                        &rel,
                        name,
                        "mechanism-prefixed function name (`sync_*`)",
                        &mut violations,
                    );
                } else if *token == "mod" && toks.get(i + 1).copied() == Some("sync") {
                    flag(
                        &rel,
                        "sync",
                        "mechanism module name (`mod sync`)",
                        &mut violations,
                    );
                }
            }
        }
    }

    for rel in sync_module_directories() {
        flag(
            &rel,
            "sync",
            "mechanism module name (`sync/` directory)",
            &mut violations,
        );
    }

    support::report(
        "naming guard: a type, function or module names the thing it is, not the \
         mechanism it uses.",
        &violations,
    );
}

#[test]
fn no_retired_names() {
    let mut violations: Vec<String> = Vec::new();

    for (rel, path) in support::source_files(&["src", "tests", "benches"]) {
        let content = support::read(&path);
        for (i, raw_line) in content.lines().enumerate() {
            let n = i + 1;
            let stripped = strip_comments_and_strings(raw_line);
            let toks = tokens(&stripped);
            for (index, keyword) in toks.iter().enumerate() {
                if !RETIRED_DECL_KEYWORDS.contains(keyword) {
                    continue;
                }
                let Some(name) = toks.get(index + 1) else {
                    continue;
                };
                if RETIRED_NAMES.contains(name) {
                    violations.push(format!(
                        "{rel}:{n}: `{keyword} {name}` — `{name}` was retired by the \
                         Zed-vocabulary sweep"
                    ));
                }
            }
        }
    }

    support::report(
        "naming guard: a retired name stays retired — the sweep renamed it once, and a \
         new declaration may not bring the old spelling back.",
        &violations,
    );
}

const CONVENTIONS: &str = include_str!("../../../../docs/conventions.md");

fn domain_word_not_cells() -> Vec<&'static str> {
    CONVENTIONS
        .lines()
        .skip_while(|line| !line.starts_with("## 8. "))
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with("| ") && !line.starts_with("| concept "))
        .filter_map(|line| line.trim_end().trim_end_matches('|').rsplit('|').next())
        .collect()
}

fn quoted_words(cell: &str) -> impl Iterator<Item = &str> {
    cell.split('`').skip(1).step_by(2).flat_map(|span| {
        span.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '*'))
            .filter(|word| !word.is_empty())
    })
}

fn word_covers(word: &str, name: &str) -> bool {
    match word.strip_suffix('*') {
        Some(prefix) => !prefix.is_empty() && name.starts_with(prefix),
        None => word == name,
    }
}

#[test]
fn retired_names_sit_in_domain_words() {
    let cells = domain_word_not_cells();
    let violations: Vec<String> = RETIRED_NAMES
        .iter()
        .filter(|name| {
            !cells
                .iter()
                .flat_map(|cell| quoted_words(cell))
                .any(|word| word_covers(word, name))
        })
        .map(|name| {
            format!("`{name}` is in `RETIRED_NAMES` but in no §8 \"not\" cell of docs/conventions.md")
        })
        .collect();

    support::report(
        "naming guard: docs/conventions.md §8 is the one names rulebook, so every retired \
         name sits in the \"not\" column of its concept's row.",
        &violations,
    );
}

const RETIRED_WORDS: &[&str] = &[
    "seek_fraction",
    "toast_notices",
    "brand",
    "ticket",
    "boot",
    "booted",
    "cover_renderer",
    "advance_clock",
    "appearance_stepped",
    "arm_cue",
    "browse_request",
    "cached_or_drawn",
    "card_status",
    "channel_sq_err",
    "chord_for_action",
    "clock_text",
    "column_lines",
    "command_inbox",
    "content_lines",
    "content_size",
    "cue_primary",
    "deck_sender",
    "desired_bindings",
    "dispatch_arrival",
    "elapsed_of",
    "elapsed_total",
    "fit_format_chips",
    "format_chip_fit",
    "format_duration_step",
    "format_pick",
    "format_sleep_presets_label",
    "format_time",
    "format_toggle",
    "gain_in",
    "gain_out",
    "inner_search",
    "inner_transition",
    "is_decodable",
    "key_context_stack",
    "leave_the_alternate_screen",
    "looked_up",
    "match_count_line",
    "max_value_width",
    "merge_config_patch",
    "modal_frame",
    "paint_progress_text",
    "parse_config_reload",
    "playback_request",
    "probe_answer",
    "query_answer",
    "reportable_shift",
    "resend_lost",
    "retire_sink",
    "seek_reset",
    "selected_line",
    "sender_index",
    "should_paint",
    "song_title",
    "source_recovered",
    "start_decode",
    "start_preload",
    "status_color",
    "status_label",
    "swap_sink",
    "table_rows",
    "template_bindings",
    "theme_picked",
    "to_key",
    "to_minutes",
    "track_ref",
    "truncate_from_left",
    "truncate_line_to_width",
    "value_rows",
    "browse_selected",
    "load_at",
];

const RETIRED_FN_PREFIXES: &[&str] = &["adjust_"];

const RETIRED_FIELD_SUFFIXES: &[&str] = &["_bg", "_px"];

fn retired_word(name: &str) -> Option<&'static str> {
    let padded = format!("_{name}_");
    RETIRED_WORDS
        .iter()
        .find(|word| padded.contains(&format!("_{word}_")))
        .copied()
}

fn field_name(stripped: &str) -> Option<&str> {
    let trimmed = stripped.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let (head, rest) = trimmed.split_once(':')?;
    if rest.starts_with(':') {
        return None;
    }
    tokens(head).last().copied()
}

fn retired_snake_names(stripped: &str, in_struct: bool) -> Vec<String> {
    let toks = tokens(stripped);
    let fn_names = toks
        .iter()
        .zip(toks.iter().skip(1))
        .filter(|(keyword, _)| **keyword == "fn")
        .map(|(_, name)| *name);
    let fn_faults = fn_names.flat_map(|name| {
        let word = retired_word(name).map(|word| format!("fn `{name}` uses `{word}`"));
        let prefix = RETIRED_FN_PREFIXES
            .iter()
            .find(|prefix| name.starts_with(**prefix))
            .map(|prefix| format!("fn `{name}` starts with `{prefix}`"));
        word.into_iter().chain(prefix)
    });
    let field = field_name(stripped).filter(|_| in_struct);
    let field_faults = field.into_iter().flat_map(|name| {
        let word =
            retired_word(name).map(|word| format!("field `{name}` uses `{word}`"));
        let suffix = RETIRED_FIELD_SUFFIXES
            .iter()
            .find(|suffix| name.ends_with(**suffix))
            .map(|suffix| format!("field `{name}` ends with `{suffix}`"));
        word.into_iter().chain(suffix)
    });
    fn_faults.chain(field_faults).collect()
}

#[test]
fn no_retired_snake_names() {
    let mut violations: Vec<String> = Vec::new();

    for (rel, path) in support::source_files(&["src", "tests", "benches"]) {
        let content = support::read(&path);
        let mut in_struct = false;
        for (i, raw_line) in content.lines().enumerate() {
            let stripped = strip_comments_and_strings(raw_line);
            let trimmed = stripped.trim();
            if in_struct && trimmed.starts_with('}') {
                in_struct = false;
                continue;
            }
            violations.extend(
                retired_snake_names(&stripped, in_struct)
                    .into_iter()
                    .map(|fault| format!("{rel}:{}: {fault}", i + 1)),
            );
            if tokens(trimmed).contains(&"struct") && trimmed.ends_with('{') {
                in_struct = true;
            }
        }
    }

    support::report(
        "naming guard: a retired word stays retired in function, test and field names — \
         the sweep renamed it once, and new code may not bring the old spelling back.",
        &violations,
    );
}

#[test]
fn no_denied_parameter_names() {
    let mut violations: Vec<String> = Vec::new();

    for (rel, path) in support::source_files(&["src", "tests", "benches"]) {
        let geometry = GEOMETRY_FILES.contains(&rel.as_str());
        let content = support::read(&path);
        for (line, name, slots) in support::parameters(&content) {
            for parameter in slots.iter().filter_map(|slot| parameter_name(slot)) {
                if !DENIED_PARAMETERS.contains(&parameter.as_str()) {
                    continue;
                }
                if geometry && GEOMETRY_PARAMETERS.contains(&parameter.as_str()) {
                    continue;
                }
                violations.push(format!("{rel}:{line}: `{name}` takes `{parameter}`"));
            }
        }
    }

    support::report(
        "naming guard: a parameter names what it carries, not the mechanism carrying \
         it and not a placeholder.",
        &violations,
    );
}

#[test]
fn no_project_abbreviations() {
    let mut violations: Vec<String> = Vec::new();

    for (rel, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for (i, raw_line) in content.lines().enumerate() {
            let n = i + 1;
            let stripped = strip_comments_and_strings(raw_line);
            for token in tokens(&stripped) {
                let Some(reason) = denylist_reason(token) else {
                    continue;
                };
                violations.push(format!("{rel}:{n}: {reason} (found `{token}`)"));
            }
        }
    }

    support::report(
        "naming guard: project-made abbreviations are spelled out as full words.",
        &violations,
    );
}
