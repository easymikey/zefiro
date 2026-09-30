use std::num::NonZeroUsize;

use crate::{
    cmd::Cue,
    domain::{Direction, ThemeName},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingId(u16);

impl SettingId {
    #[must_use]
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingRow {
    Theme,
    Crossfade,
    Replaygain,
    OutputDevice,
    SleepPresets,
    Custom(SettingId),
}

impl SettingRow {
    #[must_use]
    pub fn all(custom: &[CustomSetting]) -> Vec<SettingRow> {
        let mut rows = Vec::new();
        match custom.split_first() {
            Some((leading, rest)) => {
                rows.push(SettingRow::Custom(leading.custom.id));
                rows.push(SettingRow::Theme);
                rows.extend(rest.iter().map(|slot| SettingRow::Custom(slot.custom.id)));
            }
            None => rows.push(SettingRow::Theme),
        }
        rows.extend(
            SETTINGS
                .iter()
                .map(|entry| entry.row)
                .filter(|row| *row != SettingRow::Theme),
        );
        rows
    }

    #[must_use]
    pub fn control(self, custom: &[CustomSetting]) -> Option<SettingControl> {
        match self {
            SettingRow::Custom(id) => custom
                .iter()
                .find(|slot| slot.custom.id == id)
                .map(|slot| SettingControl::Custom(slot.custom.control)),
            SettingRow::Theme
            | SettingRow::Crossfade
            | SettingRow::Replaygain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => SETTINGS
                .iter()
                .find(|entry| entry.row == self)
                .map(|entry| entry.control),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingEntry {
    pub row: SettingRow,
    pub control: SettingControl,
}

pub const SETTINGS: [SettingEntry; 5] = [
    SettingEntry {
        row: SettingRow::Theme,
        control: SettingControl::Ring,
    },
    SettingEntry {
        row: SettingRow::Crossfade,
        control: SettingControl::Step,
    },
    SettingEntry {
        row: SettingRow::Replaygain,
        control: SettingControl::Toggle,
    },
    SettingEntry {
        row: SettingRow::OutputDevice,
        control: SettingControl::Ring,
    },
    SettingEntry {
        row: SettingRow::SleepPresets,
        control: SettingControl::Ring,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingControl {
    Toggle,
    Step,
    Ring,
    Custom(CustomControl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomControl {
    Toggle,
    Cycle(OptionCount),
    Step(OptionCount),
}

impl CustomControl {
    #[must_use]
    pub const fn count(self) -> OptionCount {
        match self {
            CustomControl::Toggle => match OptionCount::new(2) {
                Some(count) => count,
                None => OptionCount::ONE,
            },
            CustomControl::Cycle(count) | CustomControl::Step(count) => count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionCount(NonZeroUsize);

impl OptionCount {
    pub const ONE: Self = Self(NonZeroUsize::MIN);

    #[must_use]
    pub const fn new(count: usize) -> Option<Self> {
        match NonZeroUsize::new(count) {
            Some(count) => Some(Self(count)),
            None => None,
        }
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    #[must_use]
    pub fn index(self, at: usize) -> Option<OptionIndex> {
        if at < self.get() {
            Some(OptionIndex(at))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionIndex(usize);

impl OptionIndex {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Option(OptionIndex),
    Mixed,
}

impl Choice {
    #[must_use]
    pub fn nudged(self, control: CustomControl, direction: Direction) -> OptionIndex {
        let count = control.count();
        match self {
            Choice::Mixed => clamped(count, 0),
            Choice::Option(index) => {
                let next = match control {
                    CustomControl::Toggle | CustomControl::Cycle(_) => {
                        direction.wrapped(index.get(), count.get())
                    }
                    CustomControl::Step(_) => {
                        saturated(index.get(), count.get(), direction)
                    }
                };
                clamped(count, next)
            }
        }
    }
}

fn saturated(current: usize, len: usize, direction: Direction) -> usize {
    match direction {
        Direction::Next => current.saturating_add(1).min(len.saturating_sub(1)),
        Direction::Previous => current.saturating_sub(1),
    }
}

fn clamped(count: OptionCount, position: usize) -> OptionIndex {
    count
        .index(position.min(count.get() - 1))
        .unwrap_or(OptionIndex(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomRow {
    pub id: SettingId,
    pub control: CustomControl,
    pub cue: Option<Cue>,
    pub themes: &'static [Option<ThemeName>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomSetting {
    pub custom: &'static CustomRow,
    pub choice: Choice,
}

impl SettingRow {
    #[must_use]
    pub fn first(custom: &[CustomSetting]) -> Self {
        SettingRow::all(custom)
            .first()
            .copied()
            .unwrap_or(SettingRow::Theme)
    }

    #[must_use]
    pub fn moved(self, rows: &[SettingRow], direction: Direction) -> Self {
        let current = rows.iter().position(|row| *row == self).unwrap_or(0);
        let delta = direction.sign();
        let last = rows.len().saturating_sub(1);
        let next = current.checked_add_signed(delta).unwrap_or(0).min(last);
        rows.get(next).copied().unwrap_or(self)
    }

    #[must_use]
    pub fn kept(self, rows: &[SettingRow]) -> Self {
        if rows.contains(&self) {
            self
        } else {
            rows.first().copied().unwrap_or(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{Choice, CustomControl, Direction, OptionCount, OptionIndex};

    fn option(count: usize, at: usize) -> OptionIndex {
        OptionCount::new(count).unwrap().index(at).unwrap()
    }

    struct NudgeRow {
        choice: Choice,
        control: CustomControl,
        direction: Direction,
        expected: OptionIndex,
    }

    #[rstest]
    #[case::toggle_up_from_0(NudgeRow {
        choice: Choice::Option(option(2, 0)),
        control: CustomControl::Toggle,
        direction: Direction::Next,
        expected: option(2, 1),
    })]
    #[case::toggle_down_from_0(NudgeRow {
        choice: Choice::Option(option(2, 0)),
        control: CustomControl::Toggle,
        direction: Direction::Previous,
        expected: option(2, 1),
    })]
    #[case::cycle_wraps_up_at_the_end(NudgeRow {
        choice: Choice::Option(option(3, 2)),
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 0),
    })]
    #[case::cycle_wraps_down_at_0(NudgeRow {
        choice: Choice::Option(option(3, 0)),
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 2),
    })]
    #[case::step_stops_at_the_top(NudgeRow {
        choice: Choice::Option(option(3, 2)),
        control: CustomControl::Step(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 2),
    })]
    #[case::step_stops_at_0(NudgeRow {
        choice: Choice::Option(option(3, 0)),
        control: CustomControl::Step(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 0),
    })]
    #[case::mixed_up(NudgeRow {
        choice: Choice::Mixed,
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 0),
    })]
    #[case::mixed_down(NudgeRow {
        choice: Choice::Mixed,
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 0),
    })]
    fn choice_nudges_onto_a_real_option(#[case] row: NudgeRow) {
        assert_eq!(row.choice.nudged(row.control, row.direction), row.expected);
    }

    #[test]
    fn option_count_rejects_zero_and_bounds_its_index() {
        assert_eq!(OptionCount::new(0), None);
        let count = OptionCount::new(3).unwrap();
        assert_eq!(count.index(3), None);
        assert!(count.index(2).is_some());
    }
}
