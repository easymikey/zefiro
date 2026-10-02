use std::num::NonZeroUsize;

use crate::{
    cmd::Cue,
    domain::{Direction, ThemeName, appearance_rows::AppearanceField},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingRow {
    Theme,
    Crossfade,
    ReplayGain,
    OutputDevice,
    SleepPresets,
    Appearance(AppearanceField),
}

const FIXED_ROWS: [SettingRow; 4] = [
    SettingRow::Crossfade,
    SettingRow::ReplayGain,
    SettingRow::OutputDevice,
    SettingRow::SleepPresets,
];

impl SettingRow {
    #[must_use]
    pub fn all(appearance: &[AppearanceSetting]) -> Vec<SettingRow> {
        let appearance_row =
            |slot: &AppearanceSetting| SettingRow::Appearance(slot.row.field);
        let (leading, rest) = match appearance.split_first() {
            Some((leading, rest)) => (Some(appearance_row(leading)), rest),
            None => (None, appearance),
        };
        leading
            .into_iter()
            .chain([SettingRow::Theme])
            .chain(rest.iter().map(appearance_row))
            .chain(FIXED_ROWS)
            .collect()
    }

    #[must_use]
    pub fn control(
        self,
        appearance: &[AppearanceSetting],
    ) -> Option<AppearanceControl> {
        match self {
            SettingRow::Appearance(field) => appearance
                .iter()
                .find(|slot| slot.row.field == field)
                .map(|slot| slot.row.control),
            SettingRow::Theme
            | SettingRow::Crossfade
            | SettingRow::ReplayGain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => None,
        }
    }

    #[must_use]
    pub fn activates(self, appearance: &[AppearanceSetting]) -> bool {
        match self {
            SettingRow::Crossfade => false,
            SettingRow::Appearance(_) => self.control(appearance).is_some(),
            SettingRow::Theme
            | SettingRow::ReplayGain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceControl {
    Toggle,
    Cycle(OptionCount),
    Step(OptionCount),
}

impl AppearanceControl {
    #[must_use]
    pub const fn count(self) -> OptionCount {
        match self {
            AppearanceControl::Toggle => match OptionCount::new(2) {
                Some(count) => count,
                None => OptionCount::ONE,
            },
            AppearanceControl::Cycle(count) | AppearanceControl::Step(count) => count,
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
    pub fn stepped(
        self,
        control: AppearanceControl,
        direction: Direction,
    ) -> OptionIndex {
        let count = control.count();
        match self {
            Choice::Mixed => clamped(count, 0),
            Choice::Option(index) => {
                let next = match control {
                    AppearanceControl::Toggle | AppearanceControl::Cycle(_) => {
                        direction.wrapped(index.get(), count.get())
                    }
                    AppearanceControl::Step(_) => {
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
pub struct AppearanceRow {
    pub field: AppearanceField,
    pub control: AppearanceControl,
    pub cue: Option<Cue>,
    pub themes: &'static [Option<ThemeName>],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppearanceSetting {
    pub row: &'static AppearanceRow,
    pub choice: Choice,
}

impl SettingRow {
    #[must_use]
    pub fn first(appearance: &[AppearanceSetting]) -> Self {
        SettingRow::all(appearance)
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

    use crate::domain::{
        AppearanceControl,
        Choice,
        Direction,
        OptionCount,
        OptionIndex,
    };

    fn option(count: usize, at: usize) -> OptionIndex {
        OptionCount::new(count).unwrap().index(at).unwrap()
    }

    struct NudgeRow {
        choice: Choice,
        control: AppearanceControl,
        direction: Direction,
        expected: OptionIndex,
    }

    #[rstest]
    #[case::toggle_up_from_0(NudgeRow {
        choice: Choice::Option(option(2, 0)),
        control: AppearanceControl::Toggle,
        direction: Direction::Next,
        expected: option(2, 1),
    })]
    #[case::toggle_down_from_0(NudgeRow {
        choice: Choice::Option(option(2, 0)),
        control: AppearanceControl::Toggle,
        direction: Direction::Previous,
        expected: option(2, 1),
    })]
    #[case::cycle_wraps_up_at_the_end(NudgeRow {
        choice: Choice::Option(option(3, 2)),
        control: AppearanceControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 0),
    })]
    #[case::cycle_wraps_down_at_0(NudgeRow {
        choice: Choice::Option(option(3, 0)),
        control: AppearanceControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 2),
    })]
    #[case::step_stops_at_the_top(NudgeRow {
        choice: Choice::Option(option(3, 2)),
        control: AppearanceControl::Step(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 2),
    })]
    #[case::step_stops_at_0(NudgeRow {
        choice: Choice::Option(option(3, 0)),
        control: AppearanceControl::Step(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 0),
    })]
    #[case::mixed_up(NudgeRow {
        choice: Choice::Mixed,
        control: AppearanceControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 0),
    })]
    #[case::mixed_down(NudgeRow {
        choice: Choice::Mixed,
        control: AppearanceControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 0),
    })]
    fn choice_nudges_onto_a_real_option(#[case] row: NudgeRow) {
        assert_eq!(row.choice.stepped(row.control, row.direction), row.expected);
    }

    #[test]
    fn option_count_rejects_zero_and_bounds_its_index() {
        assert_eq!(OptionCount::new(0), None);
        let count = OptionCount::new(3).unwrap();
        assert_eq!(count.index(3), None);
        assert!(count.index(2).is_some());
    }
}
