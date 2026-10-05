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
    (
        "Moment",
        &["now", "played_at", "raised_at", "first", "last", "clock"],
    ),
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
        ],
    ),
    ("PathBuf", &["path", "dir"]),
    ("Path", &["path", "dir"]),
    ("Sender", &["sender", "inbox"]),
    ("Receiver", &["receiver", "doorbell"]),
    ("CoverMode", &["cover_mode"]),
    ("CrossfadePermit", &["crossfade_permit"]),
    ("CoverCrossfade", &["crossfade"]),
    ("Percent", &["volume"]),
];

pub(crate) const ERROR_WORDS: &[&str] = &["error", "source", "skipped"];

pub(crate) const COLLECTION_ROLES: &[(&str, &[&str])] = &[
    ("Duration", &["sleep_presets", "presets"]),
    ("ViewIndex", &["matches", "order"]),
    ("Scheduled", &["scheduled"]),
    ("TrackRef", &["queue"]),
    ("TrackSource", &["queue"]),
    ("HistoryEntry", &["history"]),
    ("Track", &["playlist", "tracks"]),
];

pub(crate) const RESERVED: &[(&str, &[&str])] = &[
    ("position", &["Duration"]),
    ("duration", &["Duration"]),
    ("elapsed", &["Duration"]),
    ("delay", &["Duration"]),
    ("playhead", &["Playhead"]),
    ("index", &["*Index"]),
    ("row", &["*Row", "RowIndex"]),
    ("selected", &["ViewIndex", "RowIndex", "SettingRow"]),
    ("volume", &["Percent", "*Volume"]),
    ("speed", &["Speed"]),
    ("revision", &["Revision"]),
    ("gain", &["Gain"]),
    ("decibels", &["Decibels"]),
    ("track", &["Track", "*Track"]),
    ("queue", &["TrackRef", "TrackSource"]),
    ("cursor", &["Cursor", "*Cursor"]),
    ("preloaded", &["Track"]),
    ("level", &["ToastLevel", "*Level"]),
    ("device", &["*Device", "DeviceChoice"]),
    ("choice", &["*Choice"]),
    ("cover", &["Cover*", "*Cover"]),
];
