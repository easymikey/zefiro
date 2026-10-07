pub(crate) const CONSTRUCTORS: &[&str] = &[
    "new", "default", "at", "of", "empty", "idle", "none", "parse", "open", "clamped",
    "anchored", "stock",
];

pub(crate) const CONSTRUCTOR_PREFIXES: &[&str] = &["from_", "with_", "for_"];

pub(crate) const WRAPPERS: &[&str] = &[
    "Option",
    "Arc",
    "Rc",
    "Box",
    "Cow",
    "Cell",
    "RefCell",
    "Mutex",
    "RwLock",
    "CursorOver",
    "Memo",
];

pub(crate) const COLLECTIONS: &[&str] = &["Vec", "VecDeque", "HashSet", "BTreeSet"];

pub(crate) const EXTERNAL_CHECKED: &[&str] = &[
    "Duration", "Instant", "PathBuf", "Path", "Sender", "Receiver",
];

pub(crate) const PAIR_WORDS: &[&str] = &[
    "from", "to", "old", "new", "current", "next", "previous", "expected", "actual",
    "first", "second", "last", "before", "after", "min", "max", "mid",
];

pub(crate) const ROLE_NAMED: &[&str] = &["Rgb", "Hertz", "Kbps", "Frames", "Presence"];

const STAGES: &[&str] = &["current", "incoming", "outgoing"];

pub(crate) const ROLE_WORDS: &[(&str, &[&str])] = &[
    (
        "Duration",
        &[
            "position",
            "offset",
            "target",
            "by",
            "duration",
            "delay",
            "fade_start",
            "elapsed",
            "since_first_paint",
            "advanced_to",
            "lookahead",
            "remaining",
            "crossfade",
            "step",
            "window",
            "timeout",
            "start",
            "end",
        ],
    ),
    ("Moment", &["now", "_at", "first", "last", "clock"]),
    ("Instant", &["now", "at"]),
    ("ViewIndex", &["selected", "playing_index", "index"]),
    ("TrackIndex", &["index"]),
    ("RowIndex", &["selected", "row"]),
    ("SettingRow", &["selected"]),
    ("Track", &["preloaded", "displayed_track"]),
    ("TrackSource", &["source"]),
    ("LoadedTrack", STAGES),
    ("SinkRole", STAGES),
    ("Sink", STAGES),
    ("EnvelopeControl", STAGES),
    ("Gain", STAGES),
    ("Key", &["typed"]),
    ("Revision", &["load", "preload", "issued", "decode"]),
    ("ConfigPatch", &["later"]),
    ("AppearancePatch", &["later"]),
    (
        "Pixels",
        &["side", "canvas_side", "width", "height", "radius"],
    ),
    (
        "Cells",
        &[
            "width",
            "height",
            "min_width",
            "min_height",
            "available_width",
            "content_rows",
            "visible_rows",
        ],
    ),
    ("PathBuf", &["path", "dir"]),
    ("Path", &["path", "dir"]),
    ("Sender", &["sender", "inbox"]),
    ("Receiver", &["receiver", "doorbell"]),
    ("CoverMode", &["cover_mode"]),
    ("Percent", &["volume"]),
    ("TextRequest", &["message"]),
    ("SearchRequest", &["message"]),
];

pub(crate) const ERROR_WORDS: &[&str] = &["error", "source", "skipped"];

pub(crate) const COLLECTION_ROLES: &[(&str, &[&str])] = &[
    ("Duration", &["sleep_presets", "presets"]),
    ("ViewIndex", &["matches", "order"]),
    ("Scheduled", &["scheduled"]),
    ("TrackSource", &["queue"]),
    ("HistoryEntry", &["history"]),
    ("Track", &["playlist", "tracks"]),
    ("PathBuf", &["paths"]),
    ("*Error", &["errors"]),
];

pub(crate) const EXACT_ONLY: &[&str] = &["index", "row"];

pub(crate) const ALLOWED_SITES: &[(&str, &str, &str)] = &[
    ("kernel/src/domain/cursor.rs", "index", "usize"),
    ("kernel/src/domain/index.rs", "index", "usize"),
    ("kernel/src/domain/setting_row.rs", "index", "usize"),
    ("sifr/src/startup.rs", "volume", "u8"),
    ("widgets/src/milkdrop/field.rs", "row", "usize"),
];

pub(crate) const RESERVED: &[(&str, &[&str])] = &[
    ("position", &["Duration", "*Position"]),
    ("duration", &["Duration"]),
    ("elapsed", &["Duration"]),
    ("delay", &["Duration"]),
    ("playhead", &["Playhead"]),
    ("index", &["*Index"]),
    ("row", &["*Row", "RowIndex"]),
    (
        "selected",
        &["ViewIndex", "RowIndex", "SettingRow", "Selected"],
    ),
    ("volume", &["Percent", "*Volume"]),
    ("speed", &["Speed"]),
    ("revision", &["Revision"]),
    ("gain", &["Gain", "ReplayGain"]),
    ("decibels", &["Decibels"]),
    ("track", &["Track", "*Track"]),
    ("queue", &["TrackSource"]),
    ("cursor", &["Cursor", "*Cursor"]),
    ("preloaded", &["Track"]),
    ("level", &["ToastLevel", "*Level"]),
    ("device", &["*Device", "DeviceChoice"]),
    ("choice", &["*Choice"]),
    ("cover", &["Cover*", "*Cover"]),
];

pub(crate) fn listed(
    table: &'static [(&'static str, &'static [&'static str])],
    key: &str,
) -> impl Iterator<Item = &'static str> {
    table
        .iter()
        .filter(move |(name, _)| allows(name, key))
        .flat_map(|(_, words)| words.iter().copied())
}

fn allows(pattern: &str, name: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*') {
        name.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        name.starts_with(prefix)
    } else {
        name == pattern
    }
}

pub(crate) fn ends_in(name: &str, word: &str) -> bool {
    if word.starts_with('_') {
        name.len() > word.len() && name.ends_with(word)
    } else {
        name == word || name.ends_with(&format!("_{word}"))
    }
}

fn reserves(name: &str, word: &str) -> bool {
    if EXACT_ONLY.contains(&word) {
        name == word
    } else {
        ends_in(name, word)
    }
}

pub(crate) fn reserved_clash(
    path: &str,
    name: &str,
    base: &str,
) -> Option<&'static str> {
    if ALLOWED_SITES.contains(&(path, name, base)) {
        return None;
    }
    RESERVED
        .iter()
        .find(|(word, allowed)| {
            reserves(name, word) && !allowed.iter().any(|pattern| allows(pattern, base))
        })
        .map(|(word, _)| *word)
}

#[cfg(test)]
mod tests {
    use crate::guards::value_names::tables::{
        COLLECTION_ROLES,
        ends_in,
        listed,
        reserved_clash,
    };

    #[test]
    fn a_suffix_role_word_needs_a_word_before_it() {
        assert!(ends_in("played_at", "_at"));
        assert!(ends_in("next_frame_at", "_at"));
        assert!(!ends_in("at", "_at"));
        assert!(!ends_in("deadline", "_at"));
        assert!(ends_in("now", "now"));
    }

    #[test]
    fn a_qualified_index_or_row_passes_and_the_bare_word_needs_its_type() {
        assert_eq!(reserved_clash("a.rs", "cursor_index", "usize"), None);
        assert_eq!(reserved_clash("a.rs", "title_row", "Rect"), None);
        assert_eq!(reserved_clash("a.rs", "index", "usize"), Some("index"));
        assert_eq!(reserved_clash("a.rs", "row", "Rect"), Some("row"));
        assert_eq!(reserved_clash("a.rs", "row", "RowIndex"), None);
    }

    #[test]
    fn replay_gain_selected_and_position_types_keep_their_domain_words() {
        assert_eq!(reserved_clash("a.rs", "replay_gain", "ReplayGain"), None);
        assert_eq!(reserved_clash("a.rs", "gain", "Track"), Some("gain"));
        assert_eq!(reserved_clash("a.rs", "selected", "Selected"), None);
        assert_eq!(reserved_clash("a.rs", "selected", "Rect"), Some("selected"));
        assert_eq!(reserved_clash("a.rs", "position", "CellPosition"), None);
        assert_eq!(reserved_clash("a.rs", "position", "Position"), None);
        assert_eq!(reserved_clash("a.rs", "position", "Rect"), Some("position"));
    }

    #[test]
    fn errors_names_a_collection_of_any_error_type() {
        let words = |element| listed(COLLECTION_ROLES, element).collect::<Vec<_>>();
        assert_eq!(words("KeymapError"), ["errors"]);
        assert_eq!(words("PaintError"), ["errors"]);
        assert_eq!(words("PathBuf"), ["paths"]);
        assert!(words("Keymap").is_empty());
    }

    #[test]
    fn an_allowed_site_passes_only_at_its_own_path_name_and_type() {
        let path = "kernel/src/domain/cursor.rs";
        assert_eq!(reserved_clash(path, "index", "usize"), None);
        assert_eq!(reserved_clash(path, "index", "u8"), Some("index"));
        assert_eq!(reserved_clash(path, "row", "usize"), Some("row"));
        assert_eq!(
            reserved_clash("kernel/src/other.rs", "index", "usize"),
            Some("index")
        );
        assert_eq!(reserved_clash("sifr/src/startup.rs", "volume", "u8"), None);
        assert_eq!(
            reserved_clash("widgets/src/milkdrop/field.rs", "row", "usize"),
            None
        );
    }
}
