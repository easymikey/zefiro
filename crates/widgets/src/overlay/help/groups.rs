use std::borrow::Cow;

use kernel::{domain::Action, update::keymap::KeyBinding};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpRow {
    pub(crate) chord: String,
    pub(crate) label: Cow<'static, str>,
}

pub(crate) struct HelpGroup {
    pub(crate) title: &'static str,
    pub(crate) bindings: Vec<HelpRow>,
}

pub(crate) const COLUMN_GAP: u16 = 3;
pub(crate) const CHORD_GAP: u16 = 2;
pub(crate) const MINIMUM_DESCRIPTION: u16 = 10;

pub(crate) fn small_count_u16(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

struct HelpGroupEntry {
    title: &'static str,
    actions: &'static [(Action, &'static str)],
}

#[rustfmt::skip]
const PLAYBACK_ACTIONS: &[(Action, &str)] = &[
    (Action::PlayPause, "Play / pause"),
    (Action::Next, "Next track"),
    (Action::Previous, "Previous track"),
    (Action::SeekBack, "Seek back 10s"),
    (Action::SeekForward, "Seek forward 10s"),
    (Action::SeekBackShort, "Seek back 5s"),
    (Action::SeekForwardShort, "Seek forward 5s"),
    (Action::SeekBackLong, "Seek back 30s"),
    (Action::SeekForwardLong, "Seek forward 30s"),
    (Action::VolumeUp, "Volume up"),
    (Action::VolumeDown, "Volume down"),
    (Action::Shuffle, "Toggle shuffle"),
    (Action::Repeat, "Cycle repeat"),
    (Action::SleepTimer, "Sleep timer"),
    (Action::AbRepeat, "A-B repeat"),
    (Action::SpeedDown, "Speed down"),
    (Action::SpeedUp, "Speed up"),
    (Action::JumpToTime, "Jump to time"),
    (Action::SeekTenth(0), "Seek to 0×10%"),
    (Action::SeekTenth(1), "Seek to 1×10%"),
    (Action::SeekTenth(2), "Seek to 2×10%"),
    (Action::SeekTenth(3), "Seek to 3×10%"),
    (Action::SeekTenth(4), "Seek to 4×10%"),
    (Action::SeekTenth(5), "Seek to 5×10%"),
    (Action::SeekTenth(6), "Seek to 6×10%"),
    (Action::SeekTenth(7), "Seek to 7×10%"),
    (Action::SeekTenth(8), "Seek to 8×10%"),
    (Action::SeekTenth(9), "Seek to 9×10%"),
];

#[rustfmt::skip]
const NAVIGATION_ACTIONS: &[(Action, &str)] = &[
    (Action::Down, "Down"),
    (Action::Up, "Up"),
    (Action::Top, "Top"),
    (Action::Bottom, "Bottom"),
    (Action::PageDown, "Page down"),
    (Action::PageUp, "Page up"),
    (Action::PlaySelected, "Play selected"),
];

#[rustfmt::skip]
const PLAYLIST_ACTIONS: &[(Action, &str)] = &[
    (Action::Enqueue, "Queue"),
    (Action::PlayNext, "Play next"),
    (Action::Dequeue, "Remove from queue"),
    (Action::QueueMoveUp, "Move up in queue"),
    (Action::QueueMoveDown, "Move down in queue"),
    (Action::CycleSort, "Cycle sort"),
    (Action::Favorite, "Favorite"),
    (Action::Delete, "Delete (asks first)"),
    (Action::SavePlaylist, "Save playlist"),
    (Action::FullScan, "Rescan"),
    (Action::TrackDetails, "Track info"),
];

#[rustfmt::skip]
const GENERAL_ACTIONS: &[(Action, &str)] = &[
    (Action::Search, "Find"),
    (Action::History, "History"),
    (Action::Settings, "Settings"),
    (Action::MusicDir, "Library folder"),
    (Action::Help, "Toggle this help"),
    (Action::Quit, "Quit"),
];

const HELP_GROUPS: [HelpGroupEntry; 4] = [
    HelpGroupEntry {
        title: "Playback",
        actions: PLAYBACK_ACTIONS,
    },
    HelpGroupEntry {
        title: "Navigation",
        actions: NAVIGATION_ACTIONS,
    },
    HelpGroupEntry {
        title: "Playlist",
        actions: PLAYLIST_ACTIONS,
    },
    HelpGroupEntry {
        title: "General",
        actions: GENERAL_ACTIONS,
    },
];

fn chords_for_action(bindings: &[KeyBinding], action: Action) -> String {
    bindings
        .iter()
        .filter(|binding| binding.action == Some(action))
        .map(|binding| binding.pattern.to_string())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn is_digit_chord(chord: &str) -> bool {
    chord.len() == 1 && chord.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn continues_digit_run(previous: &str, current: &str) -> bool {
    is_digit_chord(previous)
        && is_digit_chord(current)
        && previous
            .parse::<u8>()
            .ok()
            .zip(current.parse::<u8>().ok())
            .is_some_and(|(previous, current)| current == previous.saturating_add(1))
}

fn collapse_digit_runs(rows: &[(String, &'static str)]) -> Vec<HelpRow> {
    rows.chunk_by(|(previous, _), (current, _)| continues_digit_run(previous, current))
        .flat_map(|run| match run {
            [(first_chord, first_help), .., (last_chord, _)] => vec![HelpRow {
                chord: format!("{first_chord}-{last_chord}"),
                label: Cow::Owned(first_help.replacen(first_chord.as_str(), "N", 1)),
            }],
            run => run
                .iter()
                .map(|(chord, help)| HelpRow {
                    chord: chord.clone(),
                    label: Cow::Borrowed(*help),
                })
                .collect(),
        })
        .collect()
}

pub(crate) struct HelpGroups {
    pub(crate) playback: HelpGroup,
    pub(crate) navigation: HelpGroup,
    pub(crate) playlist: HelpGroup,
    pub(crate) general: HelpGroup,
}

impl HelpGroups {
    pub(crate) fn new(bindings: &[KeyBinding]) -> Self {
        let make = |spec: &HelpGroupEntry| {
            let rows: Vec<(String, &'static str)> = spec
                .actions
                .iter()
                .map(|(action, help)| (chords_for_action(bindings, *action), *help))
                .collect();
            HelpGroup {
                title: spec.title,
                bindings: collapse_digit_runs(&rows),
            }
        };
        let [playback, navigation, playlist, general] = &HELP_GROUPS;
        Self {
            playback: make(playback),
            navigation: make(navigation),
            playlist: make(playlist),
            general: make(general),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use rstest::rstest;

    use crate::overlay::help::groups::{HelpRow, collapse_digit_runs};

    fn rows(pairs: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
        pairs
            .iter()
            .map(|(chord, help)| ((*chord).to_string(), *help))
            .collect()
    }

    #[rstest]
    #[case::a_full_run(
        &[
            ("0", "Seek to 0×10%"), ("1", "Seek to 1×10%"), ("2", "Seek to 2×10%"),
            ("3", "Seek to 3×10%"), ("4", "Seek to 4×10%"), ("5", "Seek to 5×10%"),
            ("6", "Seek to 6×10%"), ("7", "Seek to 7×10%"), ("8", "Seek to 8×10%"),
            ("9", "Seek to 9×10%"),
        ],
        &[("0-9", "Seek to N×10%")]
    )]
    #[case::digits_that_do_different_things(
        &[("0", "Zero"), ("5", "Five")],
        &[("0", "Zero"), ("5", "Five")]
    )]
    #[case::not_a_digit(&[("q", "Quit")], &[("q", "Quit")])]
    fn collapse_digit_runs_merges_only_a_real_run(
        #[case] given: &[(&str, &'static str)],
        #[case] expected: &[(&str, &'static str)],
    ) {
        let merged = collapse_digit_runs(&rows(given));
        let expected: Vec<HelpRow> = expected
            .iter()
            .map(|(chord, help)| HelpRow {
                chord: (*chord).to_string(),
                label: Cow::Borrowed(*help),
            })
            .collect();
        assert_eq!(merged, expected);
    }
}
