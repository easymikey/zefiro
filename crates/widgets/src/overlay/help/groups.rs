use std::{borrow::Cow, sync::Arc, time::Duration};

use kernel::{
    domain::{
        keymap::Action,
        transport::{SEEK_LARGE, SEEK_MEDIUM, SEEK_SMALL},
    },
    update::keymap::chord::KeyBinding,
};

use crate::key_hints::chords_for_action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpRow {
    pub(crate) chord: String,
    pub(crate) label: Cow<'static, str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpGroup {
    pub(crate) title: &'static str,
    pub(crate) help_rows: Vec<HelpRow>,
}

pub(crate) const COLUMN_GAP: u16 = 3;
pub(crate) const CHORD_GAP: u16 = 2;
pub(crate) const MINIMUM_DESCRIPTION: u16 = 10;

#[derive(Clone, Copy)]
enum HelpLabel {
    Text(&'static str),
    Seek(&'static str, Duration),
}

impl HelpLabel {
    fn text(self) -> Cow<'static, str> {
        match self {
            HelpLabel::Text(text) => Cow::Borrowed(text),
            HelpLabel::Seek(verb, by) => {
                Cow::Owned(format!("{verb} {}s", by.as_secs()))
            }
        }
    }
}

struct HelpGroupEntry {
    title: &'static str,
    actions: &'static [(Action, HelpLabel)],
}

#[rustfmt::skip]
const PLAYBACK_ACTIONS: &[(Action, HelpLabel)] = &[
    (Action::PlayPause, HelpLabel::Text("Play / pause")),
    (Action::Next, HelpLabel::Text("Next track")),
    (Action::Previous, HelpLabel::Text("Previous track")),
    (Action::SeekBack, HelpLabel::Seek("Seek back", SEEK_MEDIUM)),
    (Action::SeekForward, HelpLabel::Seek("Seek forward", SEEK_MEDIUM)),
    (Action::SeekBackShort, HelpLabel::Seek("Seek back", SEEK_SMALL)),
    (Action::SeekForwardShort, HelpLabel::Seek("Seek forward", SEEK_SMALL)),
    (Action::SeekBackLong, HelpLabel::Seek("Seek back", SEEK_LARGE)),
    (Action::SeekForwardLong, HelpLabel::Seek("Seek forward", SEEK_LARGE)),
    (Action::VolumeUp, HelpLabel::Text("Volume up")),
    (Action::VolumeDown, HelpLabel::Text("Volume down")),
    (Action::Shuffle, HelpLabel::Text("Toggle shuffle")),
    (Action::Repeat, HelpLabel::Text("Cycle repeat")),
    (Action::SleepTimer, HelpLabel::Text("Sleep timer")),
    (Action::AbRepeat, HelpLabel::Text("A-B repeat")),
    (Action::SpeedDown, HelpLabel::Text("Speed down")),
    (Action::SpeedUp, HelpLabel::Text("Speed up")),
    (Action::JumpToTime, HelpLabel::Text("Jump to time")),
    (Action::SeekTenth(0), HelpLabel::Text("Seek to 0×10%")),
    (Action::SeekTenth(1), HelpLabel::Text("Seek to 1×10%")),
    (Action::SeekTenth(2), HelpLabel::Text("Seek to 2×10%")),
    (Action::SeekTenth(3), HelpLabel::Text("Seek to 3×10%")),
    (Action::SeekTenth(4), HelpLabel::Text("Seek to 4×10%")),
    (Action::SeekTenth(5), HelpLabel::Text("Seek to 5×10%")),
    (Action::SeekTenth(6), HelpLabel::Text("Seek to 6×10%")),
    (Action::SeekTenth(7), HelpLabel::Text("Seek to 7×10%")),
    (Action::SeekTenth(8), HelpLabel::Text("Seek to 8×10%")),
    (Action::SeekTenth(9), HelpLabel::Text("Seek to 9×10%")),
];

#[rustfmt::skip]
const NAVIGATION_ACTIONS: &[(Action, HelpLabel)] = &[
    (Action::Down, HelpLabel::Text("Down")),
    (Action::Up, HelpLabel::Text("Up")),
    (Action::Top, HelpLabel::Text("Top")),
    (Action::Bottom, HelpLabel::Text("Bottom")),
    (Action::PageDown, HelpLabel::Text("Page down")),
    (Action::PageUp, HelpLabel::Text("Page up")),
    (Action::PlaySelected, HelpLabel::Text("Play selected")),
];

#[rustfmt::skip]
const PLAYLIST_ACTIONS: &[(Action, HelpLabel)] = &[
    (Action::Enqueue, HelpLabel::Text("Queue")),
    (Action::PlayNext, HelpLabel::Text("Play next")),
    (Action::Dequeue, HelpLabel::Text("Remove from queue")),
    (Action::QueueMoveUp, HelpLabel::Text("Move up in queue")),
    (Action::QueueMoveDown, HelpLabel::Text("Move down in queue")),
    (Action::CycleSort, HelpLabel::Text("Cycle sort")),
    (Action::Favorite, HelpLabel::Text("Favorite")),
    (Action::Delete, HelpLabel::Text("Delete (asks first)")),
    (Action::SavePlaylist, HelpLabel::Text("Save playlist")),
    (Action::FullScan, HelpLabel::Text("Rescan")),
    (Action::TrackDetails, HelpLabel::Text("Track info")),
];

#[rustfmt::skip]
const GENERAL_ACTIONS: &[(Action, HelpLabel)] = &[
    (Action::Search, HelpLabel::Text("Find")),
    (Action::History, HelpLabel::Text("History")),
    (Action::Settings, HelpLabel::Text("Settings")),
    (Action::MusicDir, HelpLabel::Text("Library folder")),
    (Action::Help, HelpLabel::Text("Toggle this help")),
    (Action::Quit, HelpLabel::Text("Quit")),
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

fn is_digit_chord(chord: &str) -> bool {
    chord.len() == 1 && chord.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn continues_digit_run(
    (previous, previous_help): &(String, Cow<'static, str>),
    (current, current_help): &(String, Cow<'static, str>),
) -> bool {
    is_digit_chord(previous)
        && is_digit_chord(current)
        && previous
            .parse::<u8>()
            .ok()
            .zip(current.parse::<u8>().ok())
            .is_some_and(|(previous, current)| current == previous.saturating_add(1))
        && previous_help.replacen(previous.as_str(), "N", 1)
            == current_help.replacen(current.as_str(), "N", 1)
}

fn collapse_digit_runs(rows: &[(String, Cow<'static, str>)]) -> Vec<HelpRow> {
    rows.chunk_by(continues_digit_run)
        .flat_map(|run| match run {
            [(first_chord, first_help), .., (last_chord, _)] => vec![HelpRow {
                chord: format!("{first_chord}-{last_chord}"),
                label: Cow::Owned(first_help.replacen(first_chord.as_str(), "N", 1)),
            }],
            run => run
                .iter()
                .map(|(chord, help)| HelpRow {
                    chord: chord.clone(),
                    label: help.clone(),
                })
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpGroups {
    pub(crate) playback_group: Arc<HelpGroup>,
    pub(crate) navigation_group: Arc<HelpGroup>,
    pub(crate) playlist_group: Arc<HelpGroup>,
    pub(crate) general_group: Arc<HelpGroup>,
}

impl HelpGroups {
    pub(crate) fn new(bindings: &[KeyBinding]) -> Self {
        let make = |spec: &HelpGroupEntry| {
            let rows: Vec<(String, Cow<'static, str>)> = spec
                .actions
                .iter()
                .map(|(action, help)| {
                    let chords = chords_for_action(bindings, *action)
                        .collect::<Vec<_>>()
                        .join(" / ");
                    (chords, help.text())
                })
                .collect();
            Arc::new(HelpGroup {
                title: spec.title,
                help_rows: collapse_digit_runs(&rows),
            })
        };
        let [playback, navigation, playlist, general] = &HELP_GROUPS;
        Self {
            playback_group: make(playback),
            navigation_group: make(navigation),
            playlist_group: make(playlist),
            general_group: make(general),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use rstest::rstest;

    use crate::overlay::help::groups::{HelpRow, collapse_digit_runs};

    fn rows(pairs: &[(&str, &'static str)]) -> Vec<(String, Cow<'static, str>)> {
        pairs
            .iter()
            .map(|(chord, help)| ((*chord).to_string(), Cow::Borrowed(*help)))
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
    #[case::consecutive_digits_that_do_different_things(
        &[("3", "Down"), ("4", "Up")],
        &[("3", "Down"), ("4", "Up")]
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
