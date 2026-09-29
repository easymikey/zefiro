// GUARD: our own identifiers are full words, never project-made abbreviations.

use std::{fs, path::Path};

use crate::guards::support::{self, Allow};

const ALLOWLIST: &[Allow] = &[];

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

const TYPE_ALLOW: &[&str] = &[];

const DENIED_PARAMETERS: &[&str] = &[
    "data", "info", "ctx", "cfg", "opts", "options", "idx", "tmp", "res", "val",
    "value", "handle", "item", "entry", "thing", "stuff", "params", "args", "props",
    "w", "h", "n", "i",
];

const GEOMETRY_FILES: &[&str] = &["widgets/src/geometry.rs", "widgets/src/node.rs"];

const GEOMETRY_PARAMETERS: &[&str] = &["x", "y", "w", "h", "value"];

const PARAM_ALLOW: &[Allow] = &[];

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
    "Props", "Input", "State", "Tuning", "Sync", "Scratch", "Slices", "Info", "Kind",
    "Type", "Inputs", "Values", "Flags", "Params", "Options", "Data", "Manager",
    "Handler", "Helper", "Util", "Utils", "Wrapper", "Holder", "Draw",
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
    "Binding",
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
    "BrowseMessage",
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
    "CrossfadeActionParams",
    "WarpParams",
    "InjectParams",
    "SettingsValues",
    "VolumeMode",
    "VolumeConfig",
    "BatchFlags",
    "MediaWorker",
    "RingBuf",
    "Notice",
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
    "QueueRequest",
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
    let mut seen_allowlist: Vec<&str> = Vec::new();

    let mut flag =
        |rel: &str, name: &str, reason: &str, violations: &mut Vec<String>| {
            let key = format!("{rel}:{name}");
            if let Some(entry) = TYPE_ALLOW.iter().find(|e| **e == key) {
                seen_allowlist.push(*entry);
            } else {
                violations.push(format!("{rel}: {reason} (found `{name}`)"));
            }
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

    let stale: Vec<String> = TYPE_ALLOW
        .iter()
        .filter(|entry| !seen_allowlist.contains(entry))
        .map(|entry| (*entry).to_owned())
        .collect();

    support::report(
        "naming guard: a type, function or module names the thing it is, not the \
         mechanism it uses.",
        &violations,
        &stale,
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
        &[],
    );
}

#[test]
fn no_denied_parameter_names() {
    let mut violations: Vec<String> = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

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
                if let Some(row) = PARAM_ALLOW
                    .iter()
                    .find(|row| row.path == rel && row.pattern == parameter)
                {
                    seen.push((rel.clone(), row.pattern));
                } else {
                    violations
                        .push(format!("{rel}:{line}: `{name}` takes `{parameter}`"));
                }
            }
        }
    }

    support::report(
        "naming guard: a parameter names what it carries, not the mechanism carrying \
         it and not a placeholder.",
        &violations,
        &support::stale(PARAM_ALLOW, &seen),
    );
}

#[test]
fn no_project_abbreviations() {
    let mut violations: Vec<String> = Vec::new();
    let mut seen: Vec<(String, &'static str)> = Vec::new();

    for (rel, path) in support::source_files(&["src"]) {
        let content = support::read(&path);
        for (i, raw_line) in content.lines().enumerate() {
            let n = i + 1;
            let stripped = strip_comments_and_strings(raw_line);
            for token in tokens(&stripped) {
                let Some(reason) = denylist_reason(token) else {
                    continue;
                };
                if let Some(row) = ALLOWLIST
                    .iter()
                    .find(|row| row.path == rel && row.pattern == token)
                {
                    seen.push((rel.clone(), row.pattern));
                } else {
                    violations.push(format!("{rel}:{n}: {reason} (found `{token}`)"));
                }
            }
        }
    }

    support::report(
        "naming guard: project-made abbreviations are spelled out as full words.",
        &violations,
        &support::stale(ALLOWLIST, &seen),
    );
}
