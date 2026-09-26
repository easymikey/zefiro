use std::num::NonZeroUsize;

use crate::{
    cmd::Cue,
    domain::{Nudge, ThemeName, overlay::SettingsCursor},
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
                rows.push(SettingRow::Custom(leading.spec.id));
                rows.push(SettingRow::Theme);
                rows.extend(rest.iter().map(|slot| SettingRow::Custom(slot.spec.id)));
            }
            None => rows.push(SettingRow::Theme),
        }
        rows.extend(
            SETTINGS
                .iter()
                .map(|spec| spec.row)
                .filter(|row| *row != SettingRow::Theme),
        );
        rows
    }

    #[must_use]
    pub fn control(self, custom: &[CustomSetting]) -> Option<SettingControl> {
        match self {
            SettingRow::Custom(id) => custom
                .iter()
                .find(|slot| slot.spec.id == id)
                .map(|slot| SettingControl::Custom(slot.spec.control)),
            SettingRow::Theme
            | SettingRow::Crossfade
            | SettingRow::Replaygain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => SETTINGS
                .iter()
                .find(|spec| spec.row == self)
                .map(|spec| spec.control),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    pub row: SettingRow,
    pub control: SettingControl,
}

pub const SETTINGS: [SettingSpec; 5] = [
    SettingSpec {
        row: SettingRow::Theme,
        control: SettingControl::Ring,
    },
    SettingSpec {
        row: SettingRow::Crossfade,
        control: SettingControl::Step,
    },
    SettingSpec {
        row: SettingRow::Replaygain,
        control: SettingControl::Toggle,
    },
    SettingSpec {
        row: SettingRow::OutputDevice,
        control: SettingControl::Ring,
    },
    SettingSpec {
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
    pub fn nudged(self, control: CustomControl, nudge: Nudge) -> OptionIndex {
        let count = control.count();
        match self {
            Choice::Mixed => clamped(count, 0),
            Choice::Option(index) => {
                let next = match control {
                    CustomControl::Toggle | CustomControl::Cycle(_) => {
                        wrapped(index.get(), count.get(), nudge)
                    }
                    CustomControl::Step(_) => {
                        saturated(index.get(), count.get(), nudge)
                    }
                };
                clamped(count, next)
            }
        }
    }
}

fn wrapped(current: usize, len: usize, nudge: Nudge) -> usize {
    let delta: isize = match nudge {
        Nudge::Up => 1,
        Nudge::Down => -1,
    };
    let (Ok(len), Ok(current)) =
        (isize::try_from(len.max(1)), isize::try_from(current))
    else {
        return current;
    };
    let wrapped = (current + delta).rem_euclid(len);
    usize::try_from(wrapped).unwrap_or(0)
}

fn saturated(current: usize, len: usize, nudge: Nudge) -> usize {
    match nudge {
        Nudge::Up => current.saturating_add(1).min(len.saturating_sub(1)),
        Nudge::Down => current.saturating_sub(1),
    }
}

fn clamped(count: OptionCount, position: usize) -> OptionIndex {
    count
        .index(position.min(count.get() - 1))
        .unwrap_or(OptionIndex(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomSpec {
    pub id: SettingId,
    pub control: CustomControl,
    pub cue: Option<Cue>,
    pub themes: &'static [Option<ThemeName>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomSetting {
    pub spec: &'static CustomSpec,
    pub choice: Choice,
}

impl SettingsCursor {
    #[must_use]
    pub fn first(custom: &[CustomSetting]) -> Self {
        let rows = SettingRow::all(custom);
        Self {
            selected: rows.first().copied().unwrap_or(SettingRow::Theme),
        }
    }

    #[must_use]
    pub fn moved(self, rows: &[SettingRow], nudge: Nudge) -> Self {
        let current = rows
            .iter()
            .position(|row| *row == self.selected)
            .unwrap_or(0);
        let delta: isize = match nudge {
            Nudge::Up => -1,
            Nudge::Down => 1,
        };
        let last = rows.len().saturating_sub(1);
        let next = current.checked_add_signed(delta).unwrap_or(0).min(last);
        Self {
            selected: rows.get(next).copied().unwrap_or(self.selected),
        }
    }

    #[must_use]
    pub fn kept(self, rows: &[SettingRow]) -> Self {
        if rows.contains(&self.selected) {
            self
        } else {
            Self {
                selected: rows.first().copied().unwrap_or(self.selected),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{Choice, CustomControl, Nudge, OptionCount, OptionIndex};

    fn option(count: usize, at: usize) -> OptionIndex {
        OptionCount::new(count).unwrap().index(at).unwrap()
    }

    struct NudgeRow {
        choice: Choice,
        control: CustomControl,
        nudge: Nudge,
        expected: OptionIndex,
    }

    #[rstest]
    #[case::toggle_up_from_0(NudgeRow {
        choice: Choice::Option(option(2, 0)),
        control: CustomControl::Toggle,
        nudge: Nudge::Up,
        expected: option(2, 1),
    })]
    #[case::toggle_down_from_0(NudgeRow {
        choice: Choice::Option(option(2, 0)),
        control: CustomControl::Toggle,
        nudge: Nudge::Down,
        expected: option(2, 1),
    })]
    #[case::cycle_wraps_up_at_the_end(NudgeRow {
        choice: Choice::Option(option(3, 2)),
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        nudge: Nudge::Up,
        expected: option(3, 0),
    })]
    #[case::cycle_wraps_down_at_0(NudgeRow {
        choice: Choice::Option(option(3, 0)),
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        nudge: Nudge::Down,
        expected: option(3, 2),
    })]
    #[case::step_stops_at_the_top(NudgeRow {
        choice: Choice::Option(option(3, 2)),
        control: CustomControl::Step(OptionCount::new(3).unwrap()),
        nudge: Nudge::Up,
        expected: option(3, 2),
    })]
    #[case::step_stops_at_0(NudgeRow {
        choice: Choice::Option(option(3, 0)),
        control: CustomControl::Step(OptionCount::new(3).unwrap()),
        nudge: Nudge::Down,
        expected: option(3, 0),
    })]
    #[case::mixed_up(NudgeRow {
        choice: Choice::Mixed,
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        nudge: Nudge::Up,
        expected: option(3, 0),
    })]
    #[case::mixed_down(NudgeRow {
        choice: Choice::Mixed,
        control: CustomControl::Cycle(OptionCount::new(3).unwrap()),
        nudge: Nudge::Down,
        expected: option(3, 0),
    })]
    fn choice_nudges_onto_a_real_option(#[case] row: NudgeRow) {
        assert_eq!(row.choice.nudged(row.control, row.nudge), row.expected);
    }

    #[test]
    fn option_count_rejects_zero_and_bounds_its_index() {
        assert_eq!(OptionCount::new(0), None);
        let count = OptionCount::new(3).unwrap();
        assert_eq!(count.index(3), None);
        assert!(count.index(2).is_some());
    }
}
