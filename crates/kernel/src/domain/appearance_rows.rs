use crate::domain::{
    appearance::{
        Animations,
        AppearancePatch,
        AppearancePreset,
        AppearanceSettings,
        CoverBrackets,
        CoverMode,
        FormatChips,
        KeyHints,
        LayoutMode,
        ProgressTime,
        SpeedChip,
        preset_appearance,
        preset_of,
    },
    cue::Cue,
    setting_row::{
        AppearanceControl,
        AppearanceField,
        AppearanceRow,
        Choice,
        OptionCount,
        OptionIndex,
    },
};

const fn option_count(count: usize) -> OptionCount {
    match OptionCount::new(count) {
        Some(count) => count,
        None => OptionCount::ONE,
    }
}

const PRESETS: [AppearancePreset; 2] =
    [AppearancePreset::Stock, AppearancePreset::Noir];

pub const COVER_MODES: [CoverMode; 4] = [
    CoverMode::Vinyl,
    CoverMode::Plain,
    CoverMode::Milkdrop,
    CoverMode::Off,
];

pub const COVER_BRACKETS: [CoverBrackets; 2] =
    [CoverBrackets::Hidden, CoverBrackets::Shown];

pub const FORMAT_CHIPS: [FormatChips; 2] = [FormatChips::Hidden, FormatChips::Shown];

pub const SPEED_CHIPS: [SpeedChip; 3] =
    [SpeedChip::Always, SpeedChip::Changed, SpeedChip::Never];

pub const PROGRESS_TIMES: [ProgressTime; 2] =
    [ProgressTime::Elapsed, ProgressTime::Remaining];

pub const KEY_HINTS: [KeyHints; 2] = [KeyHints::Shown, KeyHints::Hidden];

pub const ANIMATIONS: [Animations; 2] = [Animations::On, Animations::Off];

pub const LAYOUT_MODES: [LayoutMode; 3] =
    [LayoutMode::Auto, LayoutMode::Full, LayoutMode::Compact];

const _: () = assert!(
    PRESETS.len() >= 2
        && COVER_MODES.len() >= 2
        && SPEED_CHIPS.len() >= 2
        && LAYOUT_MODES.len() >= 2
);

pub static APPEARANCE_ROWS: [AppearanceRow; 9] = [
    AppearanceRow {
        field: AppearanceField::Preset,
        control: AppearanceControl::Cycle(option_count(PRESETS.len())),
        cue: Some(Cue::LayoutChanged),
    },
    AppearanceRow {
        field: AppearanceField::CoverMode,
        control: AppearanceControl::Cycle(option_count(COVER_MODES.len())),
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::CoverBrackets,
        control: AppearanceControl::Toggle,
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::FormatChips,
        control: AppearanceControl::Toggle,
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::SpeedChip,
        control: AppearanceControl::Cycle(option_count(SPEED_CHIPS.len())),
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::ProgressTime,
        control: AppearanceControl::Toggle,
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::KeyHints,
        control: AppearanceControl::Toggle,
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::Animations,
        control: AppearanceControl::Toggle,
        cue: None,
    },
    AppearanceRow {
        field: AppearanceField::LayoutMode,
        control: AppearanceControl::Cycle(option_count(LAYOUT_MODES.len())),
        cue: Some(Cue::LayoutChanged),
    },
];

impl AppearanceField {
    #[must_use]
    pub fn row(self) -> &'static AppearanceRow {
        match (self, &APPEARANCE_ROWS) {
            (AppearanceField::Preset, [row, ..])
            | (AppearanceField::CoverMode, [_, row, ..])
            | (AppearanceField::CoverBrackets, [_, _, row, ..])
            | (AppearanceField::FormatChips, [_, _, _, row, ..])
            | (AppearanceField::SpeedChip, [.., row, _, _, _, _])
            | (AppearanceField::ProgressTime, [.., row, _, _, _])
            | (AppearanceField::KeyHints, [.., row, _, _])
            | (AppearanceField::Animations, [.., row, _])
            | (AppearanceField::LayoutMode, [.., row]) => row,
        }
    }
}

fn option_at<Choices: Copy, const N: usize>(
    choices: [Choices; N],
    option_index: OptionIndex,
) -> Option<Choices> {
    choices.get(option_index.get()).copied()
}

fn option_choice<Choices: Copy + PartialEq, const N: usize>(
    choices: [Choices; N],
    current: Choices,
) -> Choice {
    choices
        .into_iter()
        .position(|candidate| candidate == current)
        .and_then(|current_index| option_count(choices.len()).index(current_index))
        .map_or(Choice::Mixed, Choice::Option)
}

fn preset_choice(appearance_settings: AppearanceSettings) -> Choice {
    preset_of(appearance_settings)
        .map_or(Choice::Mixed, |preset| option_choice(PRESETS, preset))
}

pub(crate) fn field_choice(
    field: AppearanceField,
    appearance_settings: AppearanceSettings,
) -> Choice {
    match field {
        AppearanceField::Preset => preset_choice(appearance_settings),
        AppearanceField::CoverMode => {
            option_choice(COVER_MODES, appearance_settings.cover_mode)
        }
        AppearanceField::CoverBrackets => {
            option_choice(COVER_BRACKETS, appearance_settings.cover_brackets)
        }
        AppearanceField::FormatChips => {
            option_choice(FORMAT_CHIPS, appearance_settings.format_chips)
        }
        AppearanceField::SpeedChip => {
            option_choice(SPEED_CHIPS, appearance_settings.speed_chip)
        }
        AppearanceField::ProgressTime => {
            option_choice(PROGRESS_TIMES, appearance_settings.progress_time)
        }
        AppearanceField::KeyHints => {
            option_choice(KEY_HINTS, appearance_settings.key_hints)
        }
        AppearanceField::Animations => {
            option_choice(ANIMATIONS, appearance_settings.animations)
        }
        AppearanceField::LayoutMode => {
            option_choice(LAYOUT_MODES, appearance_settings.layout_mode)
        }
    }
}

#[must_use]
pub fn appearance_patch(
    field: AppearanceField,
    option_index: OptionIndex,
) -> Option<AppearancePatch> {
    Some(match field {
        AppearanceField::Preset => {
            AppearancePatch::from(preset_appearance(*PRESETS.get(option_index.get())?))
        }
        AppearanceField::CoverMode => AppearancePatch {
            cover_mode: Some(option_at(COVER_MODES, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::CoverBrackets => AppearancePatch {
            cover_brackets: Some(option_at(COVER_BRACKETS, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::FormatChips => AppearancePatch {
            format_chips: Some(option_at(FORMAT_CHIPS, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::SpeedChip => AppearancePatch {
            speed_chip: Some(option_at(SPEED_CHIPS, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::ProgressTime => AppearancePatch {
            progress_time: Some(option_at(PROGRESS_TIMES, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::KeyHints => AppearancePatch {
            key_hints: Some(option_at(KEY_HINTS, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::Animations => AppearancePatch {
            animations: Some(option_at(ANIMATIONS, option_index)?),
            ..AppearancePatch::default()
        },
        AppearanceField::LayoutMode => AppearancePatch {
            layout_mode: Some(option_at(LAYOUT_MODES, option_index)?),
            ..AppearancePatch::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;

    use crate::domain::{
        appearance::{
            AppearancePatch,
            AppearancePreset,
            AppearanceSettings,
            FormatChips,
            preset_appearance,
        },
        appearance_rows::{APPEARANCE_ROWS, appearance_patch, field_choice},
        cue::Cue,
        setting_row::{AppearanceField, Choice, OptionCount, OptionIndex},
    };

    fn option_at_row(field: AppearanceField, option_index: usize) -> OptionIndex {
        field.row().control.count().index(option_index).unwrap()
    }

    #[test]
    fn only_the_preset_and_layout_mode_rows_carry_a_cue() {
        for row in APPEARANCE_ROWS {
            if row.field == AppearanceField::LayoutMode
                || row.field == AppearanceField::Preset
            {
                assert_eq!(row.cue, Some(Cue::LayoutChanged), "{row:?}");
            } else {
                assert_eq!(row.cue, None, "{row:?}");
            }
        }
    }

    #[test]
    fn every_row_has_its_own_field() {
        let fields: HashSet<AppearanceField> =
            APPEARANCE_ROWS.iter().map(|row| row.field).collect();

        assert_eq!(fields.len(), APPEARANCE_ROWS.len());
        for row in &APPEARANCE_ROWS {
            assert_eq!(row.field.row(), row, "{row:?}");
        }
    }

    #[test]
    fn every_row_offers_as_many_options_as_its_control_counts() {
        for row in APPEARANCE_ROWS {
            let count = row.control.count();
            assert!(count.get() > 0, "{row:?} must offer at least one option");
            let last = count.index(count.get() - 1);
            assert!(last.is_some(), "{row:?}");
            assert!(count.index(count.get()).is_none(), "{row:?}");
            assert!(
                appearance_patch(row.field, last.unwrap()).is_some(),
                "{row:?}"
            );
        }
    }

    #[test]
    fn an_unknown_option_patches_nothing() {
        let option = OptionCount::new(5).unwrap().index(4).unwrap();
        assert_eq!(appearance_patch(AppearanceField::CoverMode, option), None);
    }

    #[test]
    fn field_choice_reads_the_stock_appearance_as_position_zero_for_every_row() {
        let zero = OptionCount::new(1).unwrap().index(0).unwrap();
        for row in APPEARANCE_ROWS {
            assert_eq!(
                field_choice(row.field, AppearanceSettings::default()),
                Choice::Option(zero),
                "{row:?}"
            );
        }
    }

    #[rstest]
    #[case::preset(AppearanceField::Preset, 1)]
    #[case::cover_mode(AppearanceField::CoverMode, 2)]
    #[case::cover_brackets(AppearanceField::CoverBrackets, 1)]
    #[case::format_chips(AppearanceField::FormatChips, 1)]
    #[case::speed_chip(AppearanceField::SpeedChip, 2)]
    #[case::progress_time(AppearanceField::ProgressTime, 1)]
    #[case::key_hints(AppearanceField::KeyHints, 1)]
    #[case::animations(AppearanceField::Animations, 1)]
    #[case::layout_mode(AppearanceField::LayoutMode, 2)]
    fn field_choice_is_the_inverse_of_appearance_patch(
        #[case] field: AppearanceField,
        #[case] option_index: usize,
    ) {
        let row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == field)
            .unwrap();
        let option = row.control.count().index(option_index).unwrap();
        let patch = appearance_patch(row.field, option).unwrap();
        let appearance_settings = AppearanceSettings::default().patched(patch);

        assert_eq!(
            field_choice(row.field, appearance_settings),
            Choice::Option(option)
        );
    }

    #[test]
    fn the_noir_patch_equals_the_noir_preset_field_for_field() {
        let expected = AppearancePatch {
            cover_mode: Some(preset_appearance(AppearancePreset::Noir).cover_mode),
            cover_brackets: Some(
                preset_appearance(AppearancePreset::Noir).cover_brackets,
            ),
            format_chips: Some(preset_appearance(AppearancePreset::Noir).format_chips),
            speed_chip: Some(preset_appearance(AppearancePreset::Noir).speed_chip),
            progress_time: Some(
                preset_appearance(AppearancePreset::Noir).progress_time,
            ),
            key_hints: Some(preset_appearance(AppearancePreset::Noir).key_hints),
            animations: Some(preset_appearance(AppearancePreset::Noir).animations),
            layout_mode: Some(preset_appearance(AppearancePreset::Noir).layout_mode),
        };
        let option = option_at_row(AppearanceField::Preset, 1);

        assert_eq!(
            appearance_patch(AppearanceField::Preset, option),
            Some(expected)
        );
    }

    #[test]
    fn a_noir_file_puts_preset_at_the_noir_index() {
        let option = option_at_row(AppearanceField::Preset, 1);
        let patch = appearance_patch(AppearanceField::Preset, option).unwrap();
        let appearance_settings = AppearanceSettings::default().patched(patch);

        assert_eq!(
            field_choice(AppearanceField::Preset, appearance_settings),
            Choice::Option(option)
        );
    }

    #[test]
    fn an_edited_file_puts_preset_at_custom() {
        let patch = AppearancePatch {
            format_chips: Some(FormatChips::Shown),
            ..AppearancePatch::default()
        };
        let appearance_settings = AppearanceSettings::default().patched(patch);

        assert_eq!(
            field_choice(AppearanceField::Preset, appearance_settings),
            Choice::Mixed
        );
    }
}
