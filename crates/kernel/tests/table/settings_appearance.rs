use kernel::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        appearance::{
            AppearancePatch,
            AppearancePreset,
            AppearanceSettings,
            KeyHints,
            LayoutMode,
            preset_appearance,
            preset_of,
        },
        appearance_rows::{
            COVER_MODES,
            KEY_HINTS,
            LAYOUT_MODES,
            PROGRESS_TIMES,
            SPEED_CHIPS,
        },
        cue::Cue,
        direction::Direction,
        model::Model,
        setting_row::{AppearanceControl, AppearanceField, OptionCount, SettingRow},
        theme::{ThemeChoice, ThemeName},
        time::Moment,
    },
    message::Message,
};
use rstest::rstest;

use crate::support::update::update;

fn step(model: &mut Model, row: SettingRow, direction: Direction) -> Cmd {
    update(model, Message::Step { row, direction }, Moment::default()).unwrap()
}

struct OptionRow {
    field: AppearanceField,
    directions: Vec<Direction>,
    position: fn(&AppearanceSettings) -> Option<usize>,
    expected_options: Vec<usize>,
}

#[rstest]
#[case::toggle_up_then_up_wraps(OptionRow {
    field: AppearanceField::KeyHints,
    directions: vec![Direction::Next, Direction::Next],
    position: |settings| {
        KEY_HINTS.iter().position(|&key_hints| key_hints == settings.key_hints)
    },
    expected_options: vec![1, 0],
})]
#[case::toggle_down_wraps_backward(OptionRow {
    field: AppearanceField::KeyHints,
    directions: vec![Direction::Previous],
    position: |settings| {
        KEY_HINTS.iter().position(|&key_hints| key_hints == settings.key_hints)
    },
    expected_options: vec![1],
})]
#[case::toggle_progress_time(OptionRow {
    field: AppearanceField::ProgressTime,
    directions: vec![Direction::Next, Direction::Next],
    position: |settings| {
        PROGRESS_TIMES
            .iter()
            .position(|&progress_time| progress_time == settings.progress_time)
    },
    expected_options: vec![1, 0],
})]
#[case::cycle_wraps_at_its_own_ring_size(OptionRow {
    field: AppearanceField::SpeedChip,
    directions: vec![Direction::Next; 4],
    position: |settings| {
        SPEED_CHIPS
            .iter()
            .position(|&speed_chip| speed_chip == settings.speed_chip)
    },
    expected_options: vec![1, 2, 0, 1],
})]
#[case::layout_steps_from_auto_to_compact_and_back(OptionRow {
    field: AppearanceField::LayoutMode,
    directions: vec![Direction::Next, Direction::Next],
    position: |settings| {
        LAYOUT_MODES
            .iter()
            .position(|&layout_mode| layout_mode == settings.layout_mode)
    },
    expected_options: vec![1, 0],
})]
#[case::four_options(OptionRow {
    field: AppearanceField::CoverMode,
    directions: vec![Direction::Next; 4],
    position: |settings| {
        COVER_MODES
            .iter()
            .position(|&cover_mode| cover_mode == settings.cover_mode)
    },
    expected_options: vec![1, 2, 3, 0],
})]
fn stepping_a_row_walks_its_options_and_wraps(#[case] row: OptionRow) {
    let OptionRow {
        field,
        directions,
        position,
        expected_options,
    } = row;
    let mut model = Model::default();

    let seen: Vec<Option<usize>> = directions
        .into_iter()
        .map(|direction| {
            drop(step(&mut model, SettingRow::Appearance(field), direction));
            position(&model.settings.appearance_settings)
        })
        .collect();

    let expected: Vec<Option<usize>> = expected_options.into_iter().map(Some).collect();
    assert_eq!(seen, expected);
}

#[test]
fn the_preset_and_theme_rows_lead_setting_row_all() {
    assert_eq!(
        SettingRow::ALL,
        [
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
        ]
    );
}

struct EffectsRow {
    field: AppearanceField,
    effects: Vec<Effect>,
    appearance_settings: AppearanceSettings,
}

#[rstest]
#[case::with_a_cue_the_setting_then_the_cue(EffectsRow {
    field: AppearanceField::LayoutMode,
    effects: vec![
        Effect::Config(ConfigCmd::SetAppearance(AppearancePatch {
            layout_mode: Some(LayoutMode::Compact),
            ..AppearancePatch::default()
        })),
        Effect::Animate(Cue::LayoutChanged),
    ],
    appearance_settings: AppearanceSettings {
        layout_mode: LayoutMode::Compact,
        ..AppearanceSettings::default()
    },
})]
#[case::without_a_cue_only_the_setting(EffectsRow {
    field: AppearanceField::KeyHints,
    effects: vec![Effect::Config(ConfigCmd::SetAppearance(AppearancePatch {
        key_hints: Some(KeyHints::Hidden),
        ..AppearancePatch::default()
    }))],
    appearance_settings: AppearanceSettings {
        key_hints: KeyHints::Hidden,
        ..AppearanceSettings::default()
    },
})]
fn stepping_a_row_changes_the_appearance_at_once_and_emits_its_effects(
    #[case] row: EffectsRow,
) {
    let EffectsRow {
        field,
        effects: expected,
        appearance_settings,
    } = row;
    let mut model = Model::default();

    let cmd = step(&mut model, SettingRow::Appearance(field), Direction::Next);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(effects, expected.iter().collect::<Vec<&Effect>>());
    assert_eq!(model.settings.appearance_settings, appearance_settings);
}

#[test]
fn control_reads_the_appearance_row_of_its_field() {
    let count = OptionCount::new(4).unwrap();

    assert_eq!(
        AppearanceField::CoverMode.row().control,
        AppearanceControl::Cycle(count)
    );
    assert_eq!(
        AppearanceField::Animations.row().control,
        AppearanceControl::Toggle
    );
    assert!(SettingRow::Appearance(AppearanceField::Animations).activates());
    assert!(SettingRow::ReplayGain.activates());
    assert!(!SettingRow::Crossfade.activates());
}

fn edited_appearance() -> AppearanceSettings {
    AppearanceSettings {
        key_hints: KeyHints::Hidden,
        ..AppearanceSettings::default()
    }
}

struct PresetRow {
    appearance_settings: AppearanceSettings,
    direction: Direction,
    preset: AppearancePreset,
    theme: Option<&'static str>,
}

#[rstest]
#[case::custom_steps_to_default(PresetRow {
    appearance_settings: edited_appearance(),
    direction: Direction::Next,
    preset: AppearancePreset::Stock,
    theme: None,
})]
#[case::default_steps_to_noir(PresetRow {
    appearance_settings: AppearanceSettings::default(),
    direction: Direction::Next,
    preset: AppearancePreset::Noir,
    theme: Some("noir"),
})]
#[case::noir_steps_to_default(PresetRow {
    appearance_settings: preset_appearance(AppearancePreset::Noir),
    direction: Direction::Previous,
    preset: AppearancePreset::Stock,
    theme: None,
})]
fn stepping_the_preset_row_selects_and_saves_its_options_theme(#[case] row: PresetRow) {
    let PresetRow {
        appearance_settings,
        direction,
        preset,
        theme,
    } = row;
    let field = AppearanceField::Preset;
    let mut model = Model::default();
    model.settings.appearance_settings = appearance_settings;
    let theme_name = theme.map(ThemeName::from_static);

    let cmd = step(&mut model, SettingRow::Appearance(field), direction);

    assert_eq!(preset_of(model.settings.appearance_settings), Some(preset));
    let selected = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
            return None;
        };
        Some(choice.clone())
    });
    let saved = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::Save(patch)) = effect else {
            return None;
        };
        Some(patch.clone())
    });
    assert_eq!(selected, theme_name.clone().map(ThemeChoice::Named));
    assert_eq!(
        saved,
        theme_name.clone().map(|name| ConfigPatch {
            theme_name: Some(name),
            ..ConfigPatch::default()
        })
    );
    assert_eq!(
        model.themes.theme_choice,
        theme_name.map_or(ThemeChoice::Auto, ThemeChoice::Named)
    );
}
