use kernel::{
    Cue,
    domain::{
        Choice,
        CustomControl,
        CustomRow,
        CustomSetting,
        OptionCount,
        OptionIndex,
        SettingId,
        ThemeName,
    },
};

use crate::{
    appearance::{
        Animations,
        Appearance,
        AppearancePatch,
        AppearancePreset,
        CoverBrackets,
        CoverStyle,
        FormatChips,
        KeyHints,
        LayoutMode,
        ProgressTime,
        SpeedChip,
        preset_appearance,
        preset_of,
    },
    appearance_file::AppearanceFile,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum AppearanceField {
    Preset = 12,
    CoverStyle = 0,
    CoverBrackets = 1,
    FormatChips = 2,
    SpeedChip = 3,
    ProgressRemaining = 5,
    KeyHints = 6,
    Animations = 7,
    LayoutMode = 11,
}

impl AppearanceField {
    #[must_use]
    pub const fn id(self) -> SettingId {
        SettingId::new(self as u16)
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceRow {
    pub custom: CustomRow,
    pub field: AppearanceField,
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

pub(crate) const COVER_STYLES: [CoverStyle; 4] = [
    CoverStyle::Vinyl,
    CoverStyle::Plain,
    CoverStyle::Milkdrop,
    CoverStyle::Off,
];

pub(crate) const COVER_BRACKETS: [CoverBrackets; 2] =
    [CoverBrackets::Hidden, CoverBrackets::Shown];

pub(crate) const FORMAT_CHIPS: [FormatChips; 2] =
    [FormatChips::Hidden, FormatChips::Shown];

pub(crate) const SPEED_CHIPS: [SpeedChip; 3] =
    [SpeedChip::Always, SpeedChip::Changed, SpeedChip::Never];

pub(crate) const PROGRESS_STYLES: [ProgressTime; 2] =
    [ProgressTime::Elapsed, ProgressTime::Remaining];

pub(crate) const KEY_HINTS: [KeyHints; 2] = [KeyHints::Shown, KeyHints::Hidden];

pub(crate) const ANIMATIONS: [Animations; 2] = [Animations::On, Animations::Off];

pub(crate) const LAYOUT_MODES: [LayoutMode; 3] =
    [LayoutMode::Auto, LayoutMode::Full, LayoutMode::Compact];

pub static APPEARANCE_ROWS: [AppearanceRow; 9] = [
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::Preset.id(),
            control: CustomControl::Cycle(option_count(PRESETS.len())),
            cue: Some(Cue::LayoutChanged),
            themes: &PRESET_THEMES,
        },
        field: AppearanceField::Preset,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::CoverStyle.id(),
            control: CustomControl::Cycle(option_count(COVER_STYLES.len())),
            cue: None,
            themes: &[],
        },
        field: AppearanceField::CoverStyle,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::CoverBrackets.id(),
            control: CustomControl::Toggle,
            cue: None,
            themes: &[],
        },
        field: AppearanceField::CoverBrackets,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::FormatChips.id(),
            control: CustomControl::Toggle,
            cue: None,
            themes: &[],
        },
        field: AppearanceField::FormatChips,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::SpeedChip.id(),
            control: CustomControl::Cycle(option_count(SPEED_CHIPS.len())),
            cue: None,
            themes: &[],
        },
        field: AppearanceField::SpeedChip,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::ProgressRemaining.id(),
            control: CustomControl::Toggle,
            cue: None,
            themes: &[],
        },
        field: AppearanceField::ProgressRemaining,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::KeyHints.id(),
            control: CustomControl::Toggle,
            cue: None,
            themes: &[],
        },
        field: AppearanceField::KeyHints,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::Animations.id(),
            control: CustomControl::Toggle,
            cue: None,
            themes: &[],
        },
        field: AppearanceField::Animations,
    },
    AppearanceRow {
        custom: CustomRow {
            id: AppearanceField::LayoutMode.id(),
            control: CustomControl::Cycle(option_count(LAYOUT_MODES.len())),
            cue: Some(Cue::LayoutChanged),
            themes: &[],
        },
        field: AppearanceField::LayoutMode,
    },
];

#[must_use]
pub fn appearance_row(id: SettingId) -> Option<&'static AppearanceRow> {
    APPEARANCE_ROWS.iter().find(|row| row.custom.id == id)
}

#[must_use]
pub fn custom_settings(file: &AppearanceFile) -> Vec<CustomSetting> {
    let options = file.appearance();
    APPEARANCE_ROWS
        .iter()
        .map(|row| CustomSetting {
            custom: &row.custom,
            choice: field_choice(row.field, options),
        })
        .collect()
}

fn option_at<Value: Copy, const N: usize>(
    options: [Value; N],
    option: OptionIndex,
) -> Option<Value> {
    options.get(option.get()).copied()
}

fn option_choice<Value: Copy + PartialEq, const N: usize>(
    options: [Value; N],
    value: Value,
) -> Choice {
    options
        .into_iter()
        .position(|option| option == value)
        .and_then(|at| option_count(options.len()).index(at))
        .map_or(Choice::Mixed, Choice::Option)
}

fn preset_choice(options: Appearance) -> Choice {
    preset_of(options).map_or(Choice::Mixed, |preset| option_choice(PRESETS, preset))
}

fn field_choice(field: AppearanceField, options: Appearance) -> Choice {
    match field {
        AppearanceField::Preset => preset_choice(options),
        AppearanceField::CoverStyle => option_choice(COVER_STYLES, options.cover_style),
        AppearanceField::CoverBrackets => {
            option_choice(COVER_BRACKETS, options.cover_brackets)
        }
        AppearanceField::FormatChips => {
            option_choice(FORMAT_CHIPS, options.format_chips)
        }
        AppearanceField::SpeedChip => option_choice(SPEED_CHIPS, options.speed_chip),
        AppearanceField::ProgressRemaining => {
            option_choice(PROGRESS_STYLES, options.progress_time)
        }
        AppearanceField::KeyHints => option_choice(KEY_HINTS, options.key_hints),
        AppearanceField::Animations => option_choice(ANIMATIONS, options.animations),
        AppearanceField::LayoutMode => option_choice(LAYOUT_MODES, options.layout_mode),
    }
}

fn field_patch(field: AppearanceField, option: OptionIndex) -> Option<AppearancePatch> {
    Some(match field {
        AppearanceField::Preset => {
            AppearancePatch::from(preset_appearance(*PRESETS.get(option.get())?))
        }
        AppearanceField::CoverStyle => AppearancePatch::builder()
            .cover_style(option_at(COVER_STYLES, option)?)
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
pub fn appearance_patch(id: SettingId, option: OptionIndex) -> Option<AppearancePatch> {
    field_patch(appearance_row(id)?.field, option)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use kernel::domain::{Choice, OptionCount, OptionIndex, SettingId};
    use rstest::rstest;

    use crate::{
        appearance::{
            AppearancePatch,
            AppearancePreset,
            FormatChips,
            preset_appearance,
        },
        appearance_file::AppearanceFile,
        patch::patch_appearance_text,
        rows::{
            APPEARANCE_ROWS,
            AppearanceField,
            appearance_patch,
            appearance_row,
            custom_settings,
        },
    };

    fn option_at_row(id: SettingId, position: usize) -> OptionIndex {
        appearance_row(id)
            .unwrap()
            .custom
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
                assert_eq!(row.custom.cue, Some(kernel::Cue::LayoutChanged), "{row:?}");
            } else {
                assert_eq!(row.custom.cue, None, "{row:?}");
            }
        }
    }

    #[test]
    fn custom_settings_copies_the_cue_from_its_appearance_row() {
        let rows = custom_settings(&AppearanceFile::default());
        let layout_row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == AppearanceField::LayoutMode)
            .unwrap();
        let slot = rows
            .into_iter()
            .find(|slot| slot.custom.id == layout_row.custom.id)
            .unwrap();

        assert_eq!(slot.custom.cue, Some(kernel::Cue::LayoutChanged));
    }

    #[test]
    fn every_row_has_its_own_id_and_its_own_field() {
        let ids: HashSet<u16> = APPEARANCE_ROWS
            .iter()
            .map(|row| row.custom.id.get())
            .collect();
        let fields: HashSet<AppearanceField> =
            APPEARANCE_ROWS.iter().map(|row| row.field).collect();

        assert_eq!(ids.len(), APPEARANCE_ROWS.len());
        assert_eq!(fields.len(), APPEARANCE_ROWS.len());
    }

    #[test]
    fn every_row_offers_as_many_options_as_its_control_counts() {
        for row in APPEARANCE_ROWS {
            let count = row.custom.control.count();
            assert!(count.get() > 0, "{row:?} must offer at least one option");
            let last = count.index(count.get() - 1);
            assert!(last.is_some(), "{row:?}");
            assert!(count.index(count.get()).is_none(), "{row:?}");
            assert!(
                appearance_patch(row.custom.id, last.unwrap()).is_some(),
                "{row:?}"
            );
        }
    }

    #[rstest]
    #[case::cover_style_four(
        SettingId::new(0),
        OptionCount::new(5).unwrap().index(4).unwrap(),
    )]
    #[case::unknown_row(
        SettingId::new(13),
        OptionCount::new(1).unwrap().index(0).unwrap()
    )]
    fn an_unknown_row_or_option_patches_nothing(
        #[case] id: SettingId,
        #[case] option: OptionIndex,
    ) {
        assert_eq!(appearance_patch(id, option), None);
    }

    #[rstest]
    #[case::cover_style("cover_style", 0, 3)]
    #[case::key_hints("key_hints", 6, 1)]
    #[case::layout_mode("layout_mode", 11, 2)]
    fn an_effect_lands_in_the_file_it_belongs_to(
        #[case] name: &str,
        #[case] id: u16,
        #[case] position: usize,
    ) {
        let option = option_at_row(SettingId::new(id), position);
        let patch = appearance_patch(SettingId::new(id), option).unwrap();
        let written = patch_appearance_text("", patch).unwrap();

        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(written);
        });
    }

    #[test]
    fn custom_settings_reads_the_stock_files_position_as_zero_for_every_row() {
        let rows = custom_settings(&AppearanceFile::default());
        let zero = OptionCount::new(1).unwrap().index(0).unwrap();
        assert!(
            rows.iter().all(|slot| slot.choice == Choice::Option(zero)),
            "{rows:?}"
        );
    }

    #[rstest]
    #[case::preset(AppearanceField::Preset, 1)]
    #[case::cover_style(AppearanceField::CoverStyle, 2)]
    #[case::cover_brackets(AppearanceField::CoverBrackets, 1)]
    #[case::format_chips(AppearanceField::FormatChips, 1)]
    #[case::speed_chip(AppearanceField::SpeedChip, 2)]
    #[case::progress_time(AppearanceField::ProgressRemaining, 1)]
    #[case::key_hints(AppearanceField::KeyHints, 1)]
    #[case::animations(AppearanceField::Animations, 1)]
    #[case::layout_mode(AppearanceField::LayoutMode, 2)]
    fn custom_settings_is_the_inverse_of_field_patch(
        #[case] field: AppearanceField,
        #[case] position: usize,
    ) {
        let row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == field)
            .unwrap();
        let option = row.custom.control.count().index(position).unwrap();
        let patch = appearance_patch(row.custom.id, option).unwrap();
        let file = AppearanceFile::default().patched(patch);

        let rows = custom_settings(&file);
        let slot = rows
            .into_iter()
            .find(|slot| slot.custom.id == row.custom.id)
            .unwrap();

        assert_eq!(slot.choice, Choice::Option(option));
    }

    #[test]
    fn the_noir_patch_equals_the_noir_preset_field_for_field() {
        let expected = AppearancePatch::builder()
            .cover_style(preset_appearance(AppearancePreset::Noir).cover_style)
            .cover_brackets(preset_appearance(AppearancePreset::Noir).cover_brackets)
            .format_chips(preset_appearance(AppearancePreset::Noir).format_chips)
            .speed_chip(preset_appearance(AppearancePreset::Noir).speed_chip)
            .progress_time(preset_appearance(AppearancePreset::Noir).progress_time)
            .key_hints(preset_appearance(AppearancePreset::Noir).key_hints)
            .animations(preset_appearance(AppearancePreset::Noir).animations)
            .layout_mode(preset_appearance(AppearancePreset::Noir).layout_mode)
            .build();
        let option = option_at_row(AppearanceField::Preset.id(), 1);

        assert_eq!(
            appearance_patch(AppearanceField::Preset.id(), option),
            Some(expected)
        );
    }

    #[test]
    fn a_noir_file_puts_preset_at_the_noir_index() {
        let option = option_at_row(AppearanceField::Preset.id(), 1);
        let patch = appearance_patch(AppearanceField::Preset.id(), option).unwrap();
        let file = AppearanceFile::default().patched(patch);

        let rows = custom_settings(&file);
        let slot = rows
            .into_iter()
            .find(|slot| slot.custom.id == AppearanceField::Preset.id())
            .unwrap();

        assert_eq!(slot.choice, Choice::Option(option));
    }

    #[test]
    fn an_edited_file_puts_preset_at_custom() {
        let patch = AppearancePatch::builder()
            .format_chips(FormatChips::Shown)
            .build();
        let file = AppearanceFile::default().patched(patch);

        let rows = custom_settings(&file);
        let slot = rows
            .into_iter()
            .find(|slot| slot.custom.id == AppearanceField::Preset.id())
            .unwrap();

        assert_eq!(slot.choice, Choice::Mixed);
    }

    #[test]
    fn the_preset_rows_options_carry_a_theme_per_preset() {
        let row = appearance_row(AppearanceField::Preset.id()).unwrap();

        assert_eq!(
            row.custom.themes,
            &[None, Some(kernel::domain::ThemeName::from_static("noir"))]
        );
    }
}
