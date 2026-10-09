use ratatui::symbols::block;

pub(crate) const TITLE_SEPARATOR: &str = " ── ";
pub(crate) const DOT_SEPARATOR: &str = " · ";
pub(crate) const ELLIPSIS: &str = "\u{2026}";
pub(crate) const OFFLINE_GLYPH: &str = "○";
pub(crate) const CREDENTIALS_GLYPH: &str = "!";

pub(crate) mod scrollbar {
    use ratatui::symbols::{block, scrollbar as ratatui_scrollbar, shade};

    pub(crate) const UP: &str = ratatui_scrollbar::DOUBLE_VERTICAL.begin;
    pub(crate) const DOWN: &str = ratatui_scrollbar::DOUBLE_VERTICAL.end;
    pub(crate) const TRACK: &str = shade::LIGHT;
    pub(crate) const THUMB: &str = block::FULL;
}

pub(crate) mod playlist {
    pub(crate) const PLAYING: &str = "▶";
    pub(crate) const FAVORITE: &str = "★";
    pub(crate) const QUEUED: &str = "q";
}

pub(crate) mod progress_line {
    use ratatui::symbols::line;

    pub(crate) const FULL: &str = line::THICK_HORIZONTAL;
    pub(crate) const PARTIAL: &str = "╸";
    pub(crate) const EMPTY: &str = line::HORIZONTAL;
}

pub(crate) const VOLUME_BLOCK: &str = block::FULL;

pub(crate) mod speed_chip {
    pub(crate) const MULTIPLY: char = '\u{00D7}';
    pub(crate) const MARKER: &str = "\u{00BB} ";
    pub(crate) const GAP: &str = "  ";
}

pub(crate) mod corner {
    pub(crate) const TOP_LEFT: char = '⌜';
    pub(crate) const TOP_RIGHT: char = '⌝';
    pub(crate) const BOTTOM_LEFT: char = '⌞';
    pub(crate) const BOTTOM_RIGHT: char = '⌟';
}

pub(crate) mod chip {
    pub(crate) const OPEN: &str = "[";
    pub(crate) const CLOSE: &str = "]";
    pub(crate) const OPEN_PAD: &str = "[ ";
    pub(crate) const PAD_CLOSE: &str = " ]";
}

pub(crate) mod key_hints {
    pub(crate) const SEPARATOR: &str = " ";
    pub(crate) const LABEL_GAP: &str = " ";
}

pub(crate) mod help {
    pub(crate) const OVERFLOW_HINT: &str = "↓ more";
}

pub(crate) mod search {
    pub(crate) const TITLE_WORD: &str = "SEARCH";
    pub(crate) const HEADER_PREFIX: &str = "/ ";
    pub(crate) const HEADER_GAP: &str = "   ";
    pub(crate) const SELECTED_MARKER: &str = "> ";
    pub(crate) const UNSELECTED_MARKER: &str = "  ";
    pub(crate) const RULE: &str = "─";
    pub(crate) const RULE_RUN: &str = "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────";
    pub(crate) const MATCH_SINGULAR: &str = "match";
    pub(crate) const MATCH_PLURAL: &str = "matches";
    pub(crate) const OF: &str = "of";
    pub(crate) const TOTAL: &str = "total";
    pub(crate) const NO_MATCHES: &str = "No matches";
}

pub(crate) mod history {
    pub(crate) const TITLE_WORD: &str = "HISTORY";
    pub(crate) const EMPTY_PLACEHOLDER: &str = "History is empty";
    pub(crate) const LABEL_SEPARATOR: &str = " — ";
    pub(crate) const TRACK_SINGULAR: &str = "track";
    pub(crate) const TRACK_PLURAL: &str = "tracks";
}

pub(crate) mod track_details {
    pub(crate) const TITLE_WORD: &str = "TRACK INFO";
    pub(crate) const HINT: &str = "any key · close";
    pub(crate) const TITLE_LABEL: &str = "TITLE ────  ";
    pub(crate) const ARTIST_LABEL: &str = "ARTIST ───  ";
    pub(crate) const ALBUM_LABEL: &str = "ALBUM ────  ";
    pub(crate) const YEAR_LABEL: &str = "YEAR  ";
    pub(crate) const TRACK_LABEL: &str = "TRACK  ";
    pub(crate) const DURATION_LABEL: &str = "DURATION  ";
    pub(crate) const FORMAT_LABEL: &str = "FORMAT  ";
    pub(crate) const PATH_LABEL: &str = "PATH  ";
    pub(crate) const SERVER_LABEL: &str = "SERVER  ";
    pub(crate) const ID_LABEL: &str = "ID  ";
    pub(crate) const MISSING: &str = "—";
    pub(crate) const TRACK_OF: &str = "/";
}

pub(crate) mod audio_format {
    pub(crate) const BITRATE_UNIT: &str = " kbps";
    pub(crate) const SAMPLE_RATE_UNIT: &str = " kHz";
}

pub(crate) mod quote {
    pub(crate) const QUOTE_OPEN: &str = "\"";
    pub(crate) const QUOTE_CLOSE: &str = "\"";
}

pub(crate) mod confirm_trash {
    pub(crate) const TITLE_WORD: &str = "MOVE TO TRASH?";
    pub(crate) const HINT: &str = "[y] yes   [n] no";
    pub(crate) const ARTIST_SEPARATOR: &str = " — ";
}

pub(crate) mod confirm_remove {
    pub(crate) const TITLE_WORD: &str = "REMOVE SERVER?";
    pub(crate) const HINT: &str = "Enter remove · Esc back";
}

pub(crate) mod jump_to_time {
    pub(crate) const TITLE_WORD: &str = "JUMP TO TIME";
    pub(crate) const HINT: &str = "Enter jump · Esc cancel";
}

pub(crate) mod music_dir {
    pub(crate) const TITLE_WORD: &str = "LIBRARY FOLDER";
    pub(crate) const HINT: &str = "Enter check · Esc cancel";
    pub(crate) const SELECTED_MARKER: &str = "> ";
    pub(crate) const UNSELECTED_MARKER: &str = "  ";
    pub(crate) const AUDIO: &str = "\u{266a} ";
    pub(crate) const PLAIN: &str = "  ";

    pub(crate) mod readable {
        pub(crate) const HINT: &str = "Enter save · Esc cancel";
    }

    pub(crate) mod denied {
        pub(crate) const HINT: &str = "Enter open Privacy & Security \u{b7} Esc cancel";
    }
}

pub(crate) mod settings {
    pub(crate) const TOGGLE_ON: &str = "\u{25c9} on";
    pub(crate) const TOGGLE_OFF: &str = "\u{25cb} off";
    pub(crate) const PICK_LEFT: &str = "\u{2039} ";
    pub(crate) const PICK_RIGHT: &str = " \u{203a}";
    pub(crate) const DURATION_UNIT: &str = " s";
    pub(crate) const SLEEP_OFF: &str = "off";
    pub(crate) const MINUTE_UNIT: &str = "m";
    pub(crate) const TITLE_WORD: &str = "SETTINGS";
    pub(crate) const OUTPUT_DEVICE_DEFAULT: &str = "System default";
}
