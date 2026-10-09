use std::num::NonZeroUsize;

use crate::domain::{cue::Cue, direction::Direction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppearanceField {
    Preset,
    CoverMode,
    CoverBrackets,
    FormatChips,
    SpeedChip,
    ProgressTime,
    KeyHints,
    Animations,
    LayoutMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingRow {
    Theme,
    Crossfade,
    ReplayGain,
    OutputDevice,
    SleepPresets,
    Appearance(AppearanceField),
}

impl SettingRow {
    pub const ALL: [SettingRow; 14] = [
        SettingRow::Appearance(AppearanceField::Preset),
        SettingRow::Theme,
        SettingRow::Appearance(AppearanceField::CoverMode),
        SettingRow::Appearance(AppearanceField::CoverBrackets),
        SettingRow::Appearance(AppearanceField::FormatChips),
        SettingRow::Appearance(AppearanceField::SpeedChip),
        SettingRow::Appearance(AppearanceField::ProgressTime),
        SettingRow::Appearance(AppearanceField::KeyHints),
        SettingRow::Appearance(AppearanceField::Animations),
        SettingRow::Appearance(AppearanceField::LayoutMode),
        SettingRow::Crossfade,
        SettingRow::ReplayGain,
        SettingRow::OutputDevice,
        SettingRow::SleepPresets,
    ];

    #[must_use]
    pub fn activates(self) -> bool {
        match self {
            SettingRow::Crossfade => false,
            SettingRow::Appearance(_)
            | SettingRow::Theme
            | SettingRow::ReplayGain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => true,
        }
    }

    #[must_use]
    pub const fn position(self) -> usize {
        match self {
            SettingRow::Appearance(AppearanceField::Preset) => 0,
            SettingRow::Theme => 1,
            SettingRow::Appearance(AppearanceField::CoverMode) => 2,
            SettingRow::Appearance(AppearanceField::CoverBrackets) => 3,
            SettingRow::Appearance(AppearanceField::FormatChips) => 4,
            SettingRow::Appearance(AppearanceField::SpeedChip) => 5,
            SettingRow::Appearance(AppearanceField::ProgressTime) => 6,
            SettingRow::Appearance(AppearanceField::KeyHints) => 7,
            SettingRow::Appearance(AppearanceField::Animations) => 8,
            SettingRow::Appearance(AppearanceField::LayoutMode) => 9,
            SettingRow::Crossfade => 10,
            SettingRow::ReplayGain => 11,
            SettingRow::OutputDevice => 12,
            SettingRow::SleepPresets => 13,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceControl {
    Toggle,
    Cycle(OptionCount),
}

impl AppearanceControl {
    #[must_use]
    pub const fn count(self) -> OptionCount {
        match self {
            AppearanceControl::Toggle => OptionCount::TWO,
            AppearanceControl::Cycle(count) => count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionCount(NonZeroUsize);

impl OptionCount {
    pub const ONE: Self = Self(NonZeroUsize::MIN);
    pub const TWO: Self = Self(NonZeroUsize::MIN.saturating_add(1));

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
    pub fn index(self, index: usize) -> Option<OptionIndex> {
        if index < self.get() {
            Some(OptionIndex(index))
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
    pub(crate) fn stepped(
        self,
        control: AppearanceControl,
        direction: Direction,
    ) -> OptionIndex {
        let count = control.count();
        match self {
            Choice::Mixed => clamped(count, 0),
            Choice::Option(index) => {
                clamped(count, direction.wrapped(index.get(), count.get()))
            }
        }
    }
}

fn clamped(count: OptionCount, requested_index: usize) -> OptionIndex {
    OptionIndex(requested_index.min(count.get() - 1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceRow {
    pub field: AppearanceField,
    pub control: AppearanceControl,
    pub cue: Option<Cue>,
}

impl SettingRow {
    #[must_use]
    pub const fn first() -> Self {
        SettingRow::Appearance(AppearanceField::Preset)
    }

    #[must_use]
    pub fn moved(self, rows: &[SettingRow], direction: Direction) -> Self {
        let current = rows.iter().position(|row| *row == self).unwrap_or(0);
        let delta = direction.sign();
        let last = rows.len().saturating_sub(1);
        let next = current.saturating_add_signed(delta).min(last);
        rows.get(next).copied().unwrap_or(self)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{
        direction::Direction,
        setting_row::{AppearanceControl, Choice, OptionCount, OptionIndex},
    };

    fn option(count: usize, index: usize) -> OptionIndex {
        OptionCount::new(count).unwrap().index(index).unwrap()
    }

    struct StepRow {
        choice: Choice,
        control: AppearanceControl,
        direction: Direction,
        expected: OptionIndex,
    }

    #[rstest]
    #[case::mixed_up(StepRow {
        choice: Choice::Mixed,
        control: AppearanceControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Next,
        expected: option(3, 0),
    })]
    #[case::mixed_down(StepRow {
        choice: Choice::Mixed,
        control: AppearanceControl::Cycle(OptionCount::new(3).unwrap()),
        direction: Direction::Previous,
        expected: option(3, 0),
    })]
    fn choice_steps_onto_a_real_option(#[case] row: StepRow) {
        assert_eq!(row.choice.stepped(row.control, row.direction), row.expected);
    }
}
