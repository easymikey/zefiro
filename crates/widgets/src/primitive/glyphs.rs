use ratatui::symbols::{block, line, scrollbar, shade};

pub(crate) const TITLE_SEPARATOR: &str = " ── ";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollbarGlyphs {
    pub up: &'static str,
    pub down: &'static str,
    pub track: &'static str,
    pub thumb: &'static str,
}

impl Default for ScrollbarGlyphs {
    fn default() -> Self {
        Self {
            up: scrollbar::DOUBLE_VERTICAL.begin,
            down: scrollbar::DOUBLE_VERTICAL.end,
            track: shade::LIGHT,
            thumb: block::FULL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlaylistGlyphs {
    pub playing: &'static str,
    pub favorite: &'static str,
    pub queued: &'static str,
}

impl Default for PlaylistGlyphs {
    fn default() -> Self {
        Self {
            playing: "▶",
            favorite: "★",
            queued: "q",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ProgressLineGlyphs {
    pub full: &'static str,
    pub partial: &'static str,
    pub empty: &'static str,
}

impl Default for ProgressLineGlyphs {
    fn default() -> Self {
        Self {
            full: line::THICK_HORIZONTAL,
            partial: "╸",
            empty: line::HORIZONTAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CardGlyphs {
    pub volume_filled: &'static str,
    pub volume_empty: &'static str,
}

impl Default for CardGlyphs {
    fn default() -> Self {
        Self {
            volume_filled: block::FULL,
            volume_empty: block::FULL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpeedChipGlyphs {
    pub multiply: char,
    pub marker: &'static str,
    pub gap: &'static str,
}

impl Default for SpeedChipGlyphs {
    fn default() -> Self {
        Self {
            multiply: '\u{00D7}',
            marker: "\u{00BB} ",
            gap: "  ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TruncateGlyphs {
    pub ellipsis: char,
}

impl Default for TruncateGlyphs {
    fn default() -> Self {
        Self {
            ellipsis: '\u{2026}',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerGlyphs {
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
}

impl Default for CornerGlyphs {
    fn default() -> Self {
        Self {
            top_left: '⌜',
            top_right: '⌝',
            bottom_left: '⌞',
            bottom_right: '⌟',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChipGlyphs {
    pub open: char,
    pub close: char,
    pub pad: char,
}

impl Default for ChipGlyphs {
    fn default() -> Self {
        Self {
            open: '[',
            close: ']',
            pad: ' ',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KeyHintsGlyphs {
    pub separator: &'static str,
    pub shade: &'static str,
}

impl Default for KeyHintsGlyphs {
    fn default() -> Self {
        Self {
            separator: " ",
            shade: shade::LIGHT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HelpGlyphs {
    pub overflow_hint: &'static str,
}

impl Default for HelpGlyphs {
    fn default() -> Self {
        Self {
            overflow_hint: "↓ more",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SearchGlyphs {
    pub title_word: &'static str,
    pub header_prefix: &'static str,
    pub cursor: &'static str,
    pub header_gap: &'static str,
    pub selected_marker: &'static str,
    pub unselected_marker: &'static str,
    pub rule: &'static str,
    pub match_singular: &'static str,
    pub match_plural: &'static str,
    pub of: &'static str,
    pub total: &'static str,
    pub no_matches: &'static str,
}

impl Default for SearchGlyphs {
    fn default() -> Self {
        Self {
            title_word: "SEARCH",
            header_prefix: "/ ",
            cursor: "_",
            header_gap: "   ",
            selected_marker: "> ",
            unselected_marker: "  ",
            rule: "─",
            match_singular: "match",
            match_plural: "matches",
            of: "of",
            total: "total",
            no_matches: "No matches",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HistoryGlyphs {
    pub title_word: &'static str,
    pub empty_placeholder: &'static str,
    pub label_separator: &'static str,
}

impl Default for HistoryGlyphs {
    fn default() -> Self {
        Self {
            title_word: "HISTORY",
            empty_placeholder: "History is empty",
            label_separator: " — ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackDetailsGlyphs {
    pub title_word: &'static str,
    pub hint: &'static str,
    pub title_label: &'static str,
    pub artist_label: &'static str,
    pub album_label: &'static str,
    pub year_label: &'static str,
    pub track_label: &'static str,
    pub duration_label: &'static str,
    pub format_label: &'static str,
    pub path_label: &'static str,
    pub leader_dash: char,
    pub gap: &'static str,
    pub missing: &'static str,
}

impl Default for TrackDetailsGlyphs {
    fn default() -> Self {
        Self {
            title_word: "TRACK INFO",
            hint: "any key · close",
            title_label: "TITLE",
            artist_label: "ARTIST",
            album_label: "ALBUM",
            year_label: "YEAR",
            track_label: "TRACK",
            duration_label: "DURATION",
            format_label: "FORMAT",
            path_label: "PATH",
            leader_dash: '─',
            gap: "  ",
            missing: "—",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConfirmDeleteGlyphs {
    pub title_word: &'static str,
    pub hint: &'static str,
    pub quote_open: char,
    pub quote_close: char,
    pub artist_separator: &'static str,
}

impl Default for ConfirmDeleteGlyphs {
    fn default() -> Self {
        Self {
            title_word: "MOVE TO TRASH?",
            hint: "[y] yes   [n] no",
            quote_open: '"',
            quote_close: '"',
            artist_separator: " — ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JumpToTimeGlyphs {
    pub title_word: &'static str,
    pub hint: &'static str,
}

impl Default for JumpToTimeGlyphs {
    fn default() -> Self {
        Self {
            title_word: "JUMP TO TIME",
            hint: "Enter jump · Esc cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceDirGlyphs {
    pub title_word: &'static str,
    pub hint: &'static str,
}

impl Default for SourceDirGlyphs {
    fn default() -> Self {
        Self {
            title_word: "LIBRARY FOLDER",
            hint: "Enter save · Esc cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsGlyphs {
    pub toggle_on: &'static str,
    pub toggle_off: &'static str,
    pub pick_left: &'static str,
    pub pick_right: &'static str,
    pub duration_unit: &'static str,
    pub title_word: &'static str,
    pub output_device_default: &'static str,
}

impl Default for SettingsGlyphs {
    fn default() -> Self {
        Self {
            toggle_on: "\u{25c9} on",
            toggle_off: "\u{25cb} off",
            pick_left: "\u{2039} ",
            pick_right: " \u{203a}",
            duration_unit: " s",
            title_word: "SETTINGS",
            output_device_default: "System default",
        }
    }
}
