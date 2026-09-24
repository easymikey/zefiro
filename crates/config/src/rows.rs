use kernel::{
    Cue,
    domain::{CustomSetting, SettingControl, SettingId},
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
        ProgressStyle,
        SpeedChipMode,
        preset_of,
        preset_options,
    },
    appearance_file::AppearanceFile,
    error::SettingRejection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppearanceField {
    Preset,
    CoverStyle,
    CoverBrackets,
    FormatChips,
    SpeedChip,
    ProgressRemaining,
    KeyHints,
    Animations,
    LayoutMode,
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceRow {
    pub id: SettingId,
    pub field: AppearanceField,
    pub control: SettingControl,
    pub cue: Option<Cue>,
    pub themes: &'static [Option<&'static str>],
}

const PRESETS: [AppearancePreset; 2] =
    [AppearancePreset::Default, AppearancePreset::Noir];

const PRESET_THEMES: [Option<&str>; 2] = [
    AppearancePreset::Default.theme(),
    AppearancePreset::Noir.theme(),
];

const COVER_STYLES: [CoverStyle; 4] = [
    CoverStyle::Vinyl,
    CoverStyle::Plain,
    CoverStyle::Milkdrop,
    CoverStyle::Off,
];

const COVER_BRACKETS: [CoverBrackets; 2] =
    [CoverBrackets::Hidden, CoverBrackets::Shown];

const FORMAT_CHIPS: [FormatChips; 2] = [FormatChips::Hidden, FormatChips::Shown];

const SPEED_CHIPS: [SpeedChipMode; 3] = [
    SpeedChipMode::Always,
    SpeedChipMode::Changed,
    SpeedChipMode::Never,
];

const PROGRESS_STYLES: [ProgressStyle; 2] =
    [ProgressStyle::Elapsed, ProgressStyle::Remaining];

const KEY_HINTS: [KeyHints; 2] = [KeyHints::Shown, KeyHints::Hidden];

const ANIMATIONS: [Animations; 2] = [Animations::On, Animations::Off];

const LAYOUT_MODES: [LayoutMode; 3] =
    [LayoutMode::Auto, LayoutMode::Full, LayoutMode::Compact];

pub const APPEARANCE_ROWS: [AppearanceRow; 9] = [
    AppearanceRow {
        id: SettingId(12),
        field: AppearanceField::Preset,
        control: SettingControl::Cycle(PRESETS.len()),
        cue: Some(Cue::LayoutChanged),
        themes: &PRESET_THEMES,
    },
    AppearanceRow {
        id: SettingId(0),
        field: AppearanceField::CoverStyle,
        control: SettingControl::Cycle(COVER_STYLES.len()),
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(1),
        field: AppearanceField::CoverBrackets,
        control: SettingControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(2),
        field: AppearanceField::FormatChips,
        control: SettingControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(3),
        field: AppearanceField::SpeedChip,
        control: SettingControl::Cycle(SPEED_CHIPS.len()),
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(5),
        field: AppearanceField::ProgressRemaining,
        control: SettingControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(6),
        field: AppearanceField::KeyHints,
        control: SettingControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(7),
        field: AppearanceField::Animations,
        control: SettingControl::Toggle,
        cue: None,
        themes: &[],
    },
    AppearanceRow {
        id: SettingId(11),
        field: AppearanceField::LayoutMode,
        control: SettingControl::Cycle(LAYOUT_MODES.len()),
        cue: Some(Cue::LayoutChanged),
        themes: &[],
    },
];

#[must_use]
pub fn appearance_row(id: SettingId) -> Option<AppearanceRow> {
    APPEARANCE_ROWS.into_iter().find(|row| row.id == id)
}

#[must_use]
pub fn custom_rows(file: &AppearanceFile) -> Vec<CustomSetting> {
    let options = file.options();
    APPEARANCE_ROWS
        .into_iter()
        .map(|row| CustomSetting {
            id: row.id,
            control: row.control,
            position: field_position(row.field, options),
            cue: row.cue,
            themes: row.themes,
        })
        .collect()
}

fn option_at<Value: Copy, const N: usize>(
    options: [Value; N],
    position: usize,
) -> Option<Value> {
    options.get(position).copied()
}

fn position_of<Value: Copy + PartialEq, const N: usize>(
    options: [Value; N],
    value: Value,
) -> usize {
    options
        .into_iter()
        .position(|option| option == value)
        .unwrap_or(0)
}

fn preset_position(options: Appearance) -> usize {
    preset_of(options)
        .and_then(|preset| PRESETS.iter().position(|candidate| *candidate == preset))
        .unwrap_or(PRESETS.len())
}

fn field_position(field: AppearanceField, options: Appearance) -> usize {
    match field {
        AppearanceField::Preset => preset_position(options),
        AppearanceField::CoverStyle => position_of(COVER_STYLES, options.cover_style),
        AppearanceField::CoverBrackets => {
            position_of(COVER_BRACKETS, options.cover_brackets)
        }
        AppearanceField::FormatChips => position_of(FORMAT_CHIPS, options.format_chips),
        AppearanceField::SpeedChip => position_of(SPEED_CHIPS, options.speed_chip),
        AppearanceField::ProgressRemaining => {
            position_of(PROGRESS_STYLES, options.progress_remaining)
        }
        AppearanceField::KeyHints => position_of(KEY_HINTS, options.key_hints),
        AppearanceField::Animations => position_of(ANIMATIONS, options.animations),
        AppearanceField::LayoutMode => position_of(LAYOUT_MODES, options.layout_mode),
    }
}

fn full_patch(appearance: Appearance) -> AppearancePatch {
    AppearancePatch::builder()
        .cover_style(appearance.cover_style)
        .cover_brackets(appearance.cover_brackets)
        .format_chips(appearance.format_chips)
        .speed_chip(appearance.speed_chip)
        .progress_remaining(appearance.progress_remaining)
        .key_hints(appearance.key_hints)
        .animations(appearance.animations)
        .layout_mode(appearance.layout_mode)
        .build()
}

fn field_patch(field: AppearanceField, position: usize) -> Option<AppearancePatch> {
    Some(match field {
        AppearanceField::Preset => full_patch(preset_options(*PRESETS.get(position)?)),
        AppearanceField::CoverStyle => AppearancePatch::builder()
            .cover_style(option_at(COVER_STYLES, position)?)
            .build(),
        AppearanceField::CoverBrackets => AppearancePatch::builder()
            .cover_brackets(option_at(COVER_BRACKETS, position)?)
            .build(),
        AppearanceField::FormatChips => AppearancePatch::builder()
            .format_chips(option_at(FORMAT_CHIPS, position)?)
            .build(),
        AppearanceField::SpeedChip => AppearancePatch::builder()
            .speed_chip(option_at(SPEED_CHIPS, position)?)
            .build(),
        AppearanceField::ProgressRemaining => AppearancePatch::builder()
            .progress_remaining(option_at(PROGRESS_STYLES, position)?)
            .build(),
        AppearanceField::KeyHints => AppearancePatch::builder()
            .key_hints(option_at(KEY_HINTS, position)?)
            .build(),
        AppearanceField::Animations => AppearancePatch::builder()
            .animations(option_at(ANIMATIONS, position)?)
            .build(),
        AppearanceField::LayoutMode => AppearancePatch::builder()
            .layout_mode(option_at(LAYOUT_MODES, position)?)
            .build(),
    })
}

pub fn appearance_patch(
    id: SettingId,
    position: usize,
) -> Result<AppearancePatch, SettingRejection> {
    let row = appearance_row(id).ok_or(SettingRejection::UnknownRow { id })?;
    field_patch(row.field, position).ok_or(SettingRejection::NoOption { id, position })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use kernel::domain::{SettingControl, SettingId};
    use rstest::rstest;

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
            ProgressStyle,
            SpeedChipMode,
            preset_options,
        },
        appearance_document::appearance_patched,
        appearance_file::AppearanceFile,
        error::SettingRejection,
        rows::{
            APPEARANCE_ROWS,
            AppearanceField,
            appearance_patch,
            appearance_row,
            custom_rows,
            preset_position,
        },
    };

    #[rstest]
    #[case::cover_style(
        0,
        2,
        AppearancePatch::builder().cover_style(CoverStyle::Milkdrop).build()
    )]
    #[case::cover_brackets(
        1,
        1,
        AppearancePatch::builder().cover_brackets(CoverBrackets::Shown).build()
    )]
    #[case::format_chips(
        2,
        1,
        AppearancePatch::builder().format_chips(FormatChips::Shown).build()
    )]
    #[case::speed_chip(
        3,
        2,
        AppearancePatch::builder().speed_chip(SpeedChipMode::Never).build()
    )]
    #[case::progress_remaining(
        5,
        1,
        AppearancePatch::builder()
            .progress_remaining(ProgressStyle::Remaining)
            .build()
    )]
    #[case::key_hints(
        6,
        1,
        AppearancePatch::builder().key_hints(KeyHints::Hidden).build()
    )]
    #[case::animations(
        7,
        1,
        AppearancePatch::builder().animations(Animations::Off).build()
    )]
    #[case::layout_mode(
        11,
        2,
        AppearancePatch::builder().layout_mode(LayoutMode::Compact).build()
    )]
    fn a_nudged_row_patches_exactly_its_own_field(
        #[case] id: u16,
        #[case] position: usize,
        #[case] patch: AppearancePatch,
    ) {
        assert_eq!(appearance_patch(SettingId(id), position), Ok(patch));
    }

    #[rstest]
    #[case::cover_style(
        0,
        AppearancePatch::builder().cover_style(CoverStyle::Vinyl).build()
    )]
    #[case::cover_brackets(
        1,
        AppearancePatch::builder().cover_brackets(CoverBrackets::Hidden).build()
    )]
    #[case::format_chips(
        2,
        AppearancePatch::builder().format_chips(FormatChips::Hidden).build()
    )]
    #[case::speed_chip(
        3,
        AppearancePatch::builder().speed_chip(SpeedChipMode::Always).build()
    )]
    #[case::progress_remaining(
        5,
        AppearancePatch::builder()
            .progress_remaining(ProgressStyle::Elapsed)
            .build()
    )]
    #[case::key_hints(
        6,
        AppearancePatch::builder().key_hints(KeyHints::Shown).build()
    )]
    #[case::animations(
        7,
        AppearancePatch::builder().animations(Animations::On).build()
    )]
    #[case::layout_mode(
        11,
        AppearancePatch::builder().layout_mode(LayoutMode::Auto).build()
    )]
    fn the_first_position_of_every_row_is_its_stock_option(
        #[case] id: u16,
        #[case] patch: AppearancePatch,
    ) {
        assert_eq!(appearance_patch(SettingId(id), 0), Ok(patch));
    }

    #[test]
    fn only_the_preset_and_layout_mode_rows_carry_a_cue() {
        for row in APPEARANCE_ROWS {
            if row.field == AppearanceField::LayoutMode
                || row.field == AppearanceField::Preset
            {
                assert_eq!(row.cue, Some(kernel::Cue::LayoutChanged), "{row:?}");
            } else {
                assert_eq!(row.cue, None, "{row:?}");
            }
        }
    }

    #[test]
    fn custom_rows_copies_the_cue_from_its_appearance_row() {
        let rows = custom_rows(&AppearanceFile::default());
        let layout_row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == AppearanceField::LayoutMode)
            .unwrap();
        let slot = rows
            .into_iter()
            .find(|slot| slot.id == layout_row.id)
            .unwrap();

        assert_eq!(slot.cue, Some(kernel::Cue::LayoutChanged));
    }

    #[test]
    fn every_row_has_its_own_id_and_its_own_field() {
        let ids: HashSet<u16> = APPEARANCE_ROWS.iter().map(|row| row.id.0).collect();
        let fields: HashSet<AppearanceField> =
            APPEARANCE_ROWS.iter().map(|row| row.field).collect();

        assert_eq!(ids.len(), APPEARANCE_ROWS.len());
        assert_eq!(fields.len(), APPEARANCE_ROWS.len());
    }

    #[test]
    fn every_row_offers_as_many_options_as_its_control_counts() {
        for row in APPEARANCE_ROWS {
            let len = match row.control {
                SettingControl::Toggle => 2,
                SettingControl::Cycle(len) => len,
                SettingControl::Step => 0,
            };
            assert!(len > 0, "{row:?} must be a toggle or a cycle");
            assert!(appearance_patch(row.id, len - 1).is_ok(), "{row:?}");
            assert_eq!(
                appearance_patch(row.id, len),
                Err(SettingRejection::NoOption {
                    id: row.id,
                    position: len
                }),
                "{row:?}"
            );
        }
    }

    #[test]
    fn an_id_no_row_claims_is_rejected() {
        assert_eq!(
            appearance_patch(SettingId(13), 0),
            Err(SettingRejection::UnknownRow { id: SettingId(13) })
        );
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
        let patch = appearance_patch(SettingId(id), position).unwrap();
        let written = appearance_patched("", patch).unwrap();

        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(written);
        });
    }

    #[rstest]
    #[case::preset(0)]
    #[case::cover_style(1)]
    #[case::cover_brackets(2)]
    #[case::format_chips(3)]
    #[case::speed_chip(4)]
    #[case::progress_remaining(5)]
    #[case::key_hints(6)]
    #[case::animations(7)]
    #[case::layout_mode(8)]
    fn custom_rows_carries_the_id_and_control_of_every_row(#[case] index: usize) {
        let rows = custom_rows(&AppearanceFile::default());
        let expected = APPEARANCE_ROWS[index];

        assert_eq!(rows[index].id, expected.id);
        assert_eq!(rows[index].control, expected.control);
    }

    #[test]
    fn custom_rows_names_exactly_the_appearance_rows() {
        assert_eq!(
            custom_rows(&AppearanceFile::default()).len(),
            APPEARANCE_ROWS.len()
        );
    }

    #[test]
    fn custom_rows_reads_the_stock_files_position_as_zero_for_every_row() {
        let rows = custom_rows(&AppearanceFile::default());
        assert!(rows.iter().all(|slot| slot.position == 0), "{rows:?}");
    }

    #[rstest]
    #[case::preset(AppearanceField::Preset, 1)]
    #[case::cover_style(AppearanceField::CoverStyle, 2)]
    #[case::cover_brackets(AppearanceField::CoverBrackets, 1)]
    #[case::format_chips(AppearanceField::FormatChips, 1)]
    #[case::speed_chip(AppearanceField::SpeedChip, 2)]
    #[case::progress_remaining(AppearanceField::ProgressRemaining, 1)]
    #[case::key_hints(AppearanceField::KeyHints, 1)]
    #[case::animations(AppearanceField::Animations, 1)]
    #[case::layout_mode(AppearanceField::LayoutMode, 2)]
    fn custom_rows_is_the_inverse_of_field_patch(
        #[case] field: AppearanceField,
        #[case] position: usize,
    ) {
        let row = APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == field)
            .unwrap();
        let patch = appearance_patch(row.id, position).unwrap();
        let file = AppearanceFile::default().patched(patch);

        let rows = custom_rows(&file);
        let slot = rows.into_iter().find(|slot| slot.id == row.id).unwrap();

        assert_eq!(slot.position, position);
    }

    fn preset_row_id() -> SettingId {
        appearance_row_id(AppearanceField::Preset)
    }

    fn appearance_row_id(field: AppearanceField) -> SettingId {
        APPEARANCE_ROWS
            .into_iter()
            .find(|row| row.field == field)
            .unwrap()
            .id
    }

    #[test]
    fn the_noir_patch_equals_the_noir_preset_field_for_field() {
        let expected = AppearancePatch::builder()
            .cover_style(preset_options(AppearancePreset::Noir).cover_style)
            .cover_brackets(preset_options(AppearancePreset::Noir).cover_brackets)
            .format_chips(preset_options(AppearancePreset::Noir).format_chips)
            .speed_chip(preset_options(AppearancePreset::Noir).speed_chip)
            .progress_remaining(
                preset_options(AppearancePreset::Noir).progress_remaining,
            )
            .key_hints(preset_options(AppearancePreset::Noir).key_hints)
            .animations(preset_options(AppearancePreset::Noir).animations)
            .layout_mode(preset_options(AppearancePreset::Noir).layout_mode)
            .build();

        assert_eq!(appearance_patch(preset_row_id(), 1), Ok(expected));
    }

    #[test]
    fn a_noir_file_puts_preset_at_the_noir_index() {
        let patch = appearance_patch(preset_row_id(), 1).unwrap();
        let file = AppearanceFile::default().patched(patch);

        let rows = custom_rows(&file);
        let slot = rows
            .into_iter()
            .find(|slot| slot.id == preset_row_id())
            .unwrap();

        assert_eq!(slot.position, 1);
    }

    #[test]
    fn an_edited_file_puts_preset_at_custom() {
        let patch = AppearancePatch::builder()
            .format_chips(FormatChips::Shown)
            .build();
        let file = AppearanceFile::default().patched(patch);

        let rows = custom_rows(&file);
        let slot = rows
            .into_iter()
            .find(|slot| slot.id == preset_row_id())
            .unwrap();

        assert_eq!(slot.position, 2);
    }

    #[test]
    fn the_preset_rows_options_carry_a_theme_per_preset() {
        let row = appearance_row(preset_row_id()).unwrap();

        assert_eq!(row.themes, &[None, Some("noir")]);
    }

    #[test]
    fn the_stock_appearance_matches_no_preset_once_edited() {
        let edited = Appearance {
            format_chips: FormatChips::Shown,
            ..Appearance::default()
        };

        assert_eq!(preset_position(edited), 2);
    }
}
