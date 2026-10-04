use crate::{
    Cue,
    domain::{
        AppearanceControl,
        AppearanceRow,
        AppearanceSetting,
        Choice,
        OptionCount,
        OptionIndex,
        ThemeName,
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
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppearanceField {
    Preset,
    CoverMode,
    CoverBrackets,
    FormatChips,
    SpeedChip,
    ProgressRemaining,
    KeyHints,
    Animations,
    LayoutMode,
}

const fn option_count(count: usize) -> OptionCount {
    match OptionCount::new(count) {
        Some(count) => count,
        None => OptionCount::ONE,
    }
}

const PRESETS: [AppearancePreset; 2] =
    [AppearancePreset::Stock, AppearancePreset::Noir];

const PRESET_THEMES: [Option<ThemeName>; 2] = [
    AppearancePreset::Stock.theme(),
    AppearancePreset::Noir.theme(),
];

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

pub const PROGRESS_STYLES: [ProgressTime; 2] =
    [ProgressTime::Elapsed, ProgressTime::Remaining];

pub const KEY_HINTS: [KeyHints; 2] = [KeyHints::Shown, KeyHints::Hidden];

pub const ANIMATIONS: [Animations; 2] = [Animations::On, Animations::Off];

pub const LAYOUT_MODES: [LayoutMode; 3] =
    [LayoutMode::Auto, LayoutMode::Full, LayoutMode::Compact];

pub static APPEARANCE_ROWS: [AppearanceRow; 9] = [
    AppearanceRow {
        field: AppearanceField::Preset,
        control: AppearanceControl::Cycle(option_count(PRESETS.len())),
        cue: Some(Cue::LayoutChanged),
        themes: &PRESET_THEMES,
    },
    AppearanceRow {
        field: AppearanceField::CoverMode,
        control: AppearanceControl::Cycle(option_count(COVER_MODES.len())),
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::CoverBrackets,
        control: AppearanceControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::FormatChips,
        control: AppearanceControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::SpeedChip,
        control: AppearanceControl::Cycle(option_count(SPEED_CHIPS.len())),
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::ProgressRemaining,
        control: AppearanceControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::KeyHints,
        control: AppearanceControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::Animations,
        control: AppearanceControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        field: AppearanceField::LayoutMode,
        control: AppearanceControl::Cycle(option_count(LAYOUT_MODES.len())),
        cue: Some(Cue::LayoutChanged),
        themes: &[],
    },
];

#[must_use]
pub fn appearance_row(id: AppearanceField) -> Option<&'static AppearanceRow> {
    APPEARANCE_ROWS.iter().find(|row| row.field == id)
}

#[must_use]
pub fn appearance_settings(appearance: AppearanceSettings) -> Vec<AppearanceSetting> {
    APPEARANCE_ROWS
        .iter()
        .map(|row| AppearanceSetting {
            row,
            choice: field_choice(row.field, appearance),
        })
        .collect()
}

fn option_at<Choices: Copy, const N: usize>(
    choices: [Choices; N],
    option: OptionIndex,
) -> Option<Choices> {
    choices.get(option.get()).copied()
}

fn option_choice<Choices: Copy + PartialEq, const N: usize>(
    choices: [Choices; N],
    current: Choices,
) -> Choice {
    choices
        .into_iter()
        .position(|candidate| candidate == current)
        .and_then(|at| option_count(choices.len()).index(at))
        .map_or(Choice::Mixed, Choice::Option)
}

fn preset_choice(appearance: AppearanceSettings) -> Choice {
    preset_of(appearance).map_or(Choice::Mixed, |preset| option_choice(PRESETS, preset))
}

fn field_choice(field: AppearanceField, appearance: AppearanceSettings) -> Choice {
    match field {
        AppearanceField::Preset => preset_choice(appearance),
        AppearanceField::CoverMode => option_choice(COVER_MODES, appearance.cover_mode),
        AppearanceField::CoverBrackets => {
            option_choice(COVER_BRACKETS, appearance.cover_brackets)
        }
        AppearanceField::FormatChips => {
            option_choice(FORMAT_CHIPS, appearance.format_chips)
        }
        AppearanceField::SpeedChip => option_choice(SPEED_CHIPS, appearance.speed_chip),
        AppearanceField::ProgressRemaining => {
            option_choice(PROGRESS_STYLES, appearance.progress_time)
        }
        AppearanceField::KeyHints => option_choice(KEY_HINTS, appearance.key_hints),
        AppearanceField::Animations => option_choice(ANIMATIONS, appearance.animations),
        AppearanceField::LayoutMode => {
            option_choice(LAYOUT_MODES, appearance.layout_mode)
        }
    }
}

fn field_patch(field: AppearanceField, option: OptionIndex) -> Option<AppearancePatch> {
    Some(match field {
        AppearanceField::Preset => {
            AppearancePatch::from(preset_appearance(*PRESETS.get(option.get())?))
        }
        AppearanceField::CoverMode => AppearancePatch::builder()
            .cover_mode(option_at(COVER_MODES, option)?)
            .build(),
        AppearanceField::CoverBrackets => AppearancePatch::builder()
            .cover_brackets(option_at(COVER_BRACKETS, option)?)
            .build(),
        AppearanceField::FormatChips => AppearancePatch::builder()
            .format_chips(option_at(FORMAT_CHIPS, option)?)
            .build(),
        AppearanceField::SpeedChip => AppearancePatch::builder()
            .speed_chip(option_at(SPEED_CHIPS, option)?)
            .build(),
        AppearanceField::ProgressRemaining => AppearancePatch::builder()
            .progress_time(option_at(PROGRESS_STYLES, option)?)
            .build(),
        AppearanceField::KeyHints => AppearancePatch::builder()
            .key_hints(option_at(KEY_HINTS, option)?)
            .build(),
        AppearanceField::Animations => AppearancePatch::builder()
            .animations(option_at(ANIMATIONS, option)?)
            .build(),
        AppearanceField::LayoutMode => AppearancePatch::builder()
            .layout_mode(option_at(LAYOUT_MODES, option)?)
            .build(),
    })
}

#[must_use]
pub fn appearance_patch(
    id: AppearanceField,
    option: OptionIndex,
) -> Option<AppearancePatch> {
    field_patch(appearance_row(id)?.field, option)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;

    use crate::{
        Cue,
        domain::{
            Choice,
            OptionCount,
            OptionIndex,
            ThemeName,
            appearance::{
                AppearancePatch,
                AppearancePreset,
                AppearanceSettings,
                FormatChips,
                preset_appearance,
            },
            appearance_rows::{
                APPEARANCE_ROWS,
                AppearanceField,
                appearance_patch,
                appearance_row,
                appearance_settings,
            },
        },
    };

    fn option_at_row(id: AppearanceField, position: usize) -> OptionIndex {
        appearance_row(id)
            .unwrap()
            .control
            .count()
            .index(position)
            .unwrap()
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
    fn appearance_settings_copies_the_cue_from_its_appearance_row() {
        let rows = appearance_settings(AppearanceSettings::default());
        let layout_row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == AppearanceField::LayoutMode)
            .unwrap();
        let slot = rows
            .into_iter()
            .find(|slot| slot.row.field == layout_row.field)
            .unwrap();

        assert_eq!(slot.row.cue, Some(Cue::LayoutChanged));
    }

    #[test]
    fn every_row_has_its_own_field() {
        let fields: HashSet<AppearanceField> =
            APPEARANCE_ROWS.iter().map(|row| row.field).collect();

        assert_eq!(fields.len(), APPEARANCE_ROWS.len());
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
    fn appearance_settings_reads_the_stock_appearance_as_position_zero_for_every_row() {
        let rows = appearance_settings(AppearanceSettings::default());
        let zero = OptionCount::new(1).unwrap().index(0).unwrap();
        assert!(
            rows.iter().all(|slot| slot.choice == Choice::Option(zero)),
            "{rows:?}"
        );
    }

    #[rstest]
    #[case::preset(AppearanceField::Preset, 1)]
    #[case::cover_mode(AppearanceField::CoverMode, 2)]
    #[case::cover_brackets(AppearanceField::CoverBrackets, 1)]
    #[case::format_chips(AppearanceField::FormatChips, 1)]
    #[case::speed_chip(AppearanceField::SpeedChip, 2)]
    #[case::progress_time(AppearanceField::ProgressRemaining, 1)]
    #[case::key_hints(AppearanceField::KeyHints, 1)]
    #[case::animations(AppearanceField::Animations, 1)]
    #[case::layout_mode(AppearanceField::LayoutMode, 2)]
    fn appearance_settings_is_the_inverse_of_field_patch(
        #[case] field: AppearanceField,
        #[case] position: usize,
    ) {
        let row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == field)
            .unwrap();
        let option = row.control.count().index(position).unwrap();
        let patch = appearance_patch(row.field, option).unwrap();
        let appearance = AppearanceSettings::default().patched(patch);

        let rows = appearance_settings(appearance);
        let slot = rows
            .into_iter()
            .find(|slot| slot.row.field == row.field)
            .unwrap();

        assert_eq!(slot.choice, Choice::Option(option));
    }

    #[test]
    fn the_noir_patch_equals_the_noir_preset_field_for_field() {
        let expected = AppearancePatch::builder()
            .cover_mode(preset_appearance(AppearancePreset::Noir).cover_mode)
            .cover_brackets(preset_appearance(AppearancePreset::Noir).cover_brackets)
            .format_chips(preset_appearance(AppearancePreset::Noir).format_chips)
            .speed_chip(preset_appearance(AppearancePreset::Noir).speed_chip)
            .progress_time(preset_appearance(AppearancePreset::Noir).progress_time)
            .key_hints(preset_appearance(AppearancePreset::Noir).key_hints)
            .animations(preset_appearance(AppearancePreset::Noir).animations)
            .layout_mode(preset_appearance(AppearancePreset::Noir).layout_mode)
            .build();
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
        let appearance = AppearanceSettings::default().patched(patch);

        let rows = appearance_settings(appearance);
        let slot = rows
            .into_iter()
            .find(|slot| slot.row.field == AppearanceField::Preset)
            .unwrap();

        assert_eq!(slot.choice, Choice::Option(option));
    }

    #[test]
    fn an_edited_file_puts_preset_at_custom() {
        let patch = AppearancePatch::builder()
            .format_chips(FormatChips::Shown)
            .build();
        let appearance = AppearanceSettings::default().patched(patch);

        let rows = appearance_settings(appearance);
        let slot = rows
            .into_iter()
            .find(|slot| slot.row.field == AppearanceField::Preset)
            .unwrap();

        assert_eq!(slot.choice, Choice::Mixed);
    }

    #[test]
    fn the_preset_rows_options_carry_a_theme_per_preset() {
        let row = appearance_row(AppearanceField::Preset).unwrap();

        assert_eq!(row.themes, &[None, Some(ThemeName::from_static("noir"))]);
    }
}
