use kernel::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        appearance::{
            AppearancePatch,
            AppearancePreset,
            AppearanceSettings,
            CoverMode,
            KeyHints,
            LayoutMode,
            preset_appearance,
            preset_of,
        },
        appearance_rows::{KEY_HINTS, SPEED_CHIPS},
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

#[rstest]
#[case::up_then_up_wraps(vec![Direction::Next, Direction::Next], vec![1, 0])]
#[case::down_wraps_backward(vec![Direction::Previous], vec![1])]
fn a_toggle_row_wraps_mod_two(
    #[case] directions: Vec<Direction>,
    #[case] expected_options: Vec<usize>,
) {
    let mut model = Model::default();
    let field = AppearanceField::KeyHints;

    let seen: Vec<usize> = directions
        .into_iter()
        .map(|direction| {
            drop(step(&mut model, SettingRow::Appearance(field), direction));
            KEY_HINTS
                .iter()
                .position(|&key_hints| {
                    key_hints == model.settings.appearance_settings.key_hints
                })
                .unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(seen, expected_options);
}

#[test]
fn a_cycle_row_wraps_at_its_own_ring_size() {
    let mut model = Model::default();
    let field = AppearanceField::SpeedChip;

    let walked: Vec<usize> = (0..4)
        .map(|_| {
            drop(step(
                &mut model,
                SettingRow::Appearance(field),
                Direction::Next,
            ));
            SPEED_CHIPS
                .iter()
                .position(|&speed_chip| {
                    speed_chip == model.settings.appearance_settings.speed_chip
                })
                .unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(walked, vec![1, 2, 0, 1]);
}

#[test]
fn the_layout_row_steps_from_auto_to_compact_and_back() {
    let mut model = Model::default();

    let layout_modes: Vec<LayoutMode> = (0..2)
        .map(|_| {
            drop(step(
                &mut model,
                SettingRow::Appearance(AppearanceField::LayoutMode),
                Direction::Next,
            ));
            model.settings.appearance_settings.layout_mode
        })
        .collect();

    assert_eq!(layout_modes, vec![LayoutMode::Compact, LayoutMode::Auto]);
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

#[rstest]
#[case::toggle(AppearanceField::ProgressTime, 2)]
#[case::cycle(AppearanceField::SpeedChip, 3)]
#[case::layout(AppearanceField::LayoutMode, 2)]
#[case::four_options(AppearanceField::CoverMode, 4)]
fn stepping_every_option_of_a_row_is_handled(
    #[case] field: AppearanceField,
    #[case] option_count: usize,
) {
    let mut model = Model::default();

    for _ in 0..option_count {
        drop(step(
            &mut model,
            SettingRow::Appearance(field),
            Direction::Next,
        ));
    }
}

#[test]
fn stepping_a_row_with_a_cue_emits_the_setting_then_the_cue() {
    let mut model = Model::default();

    let cmd = step(
        &mut model,
        SettingRow::Appearance(AppearanceField::LayoutMode),
        Direction::Next,
    );

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![
            &Effect::Config(ConfigCmd::SetAppearance(AppearancePatch {
                layout_mode: Some(LayoutMode::Compact),
                ..AppearancePatch::default()
            })),
            &Effect::Animate(Cue::LayoutChanged),
        ]
    );
}

#[test]
fn stepping_a_row_without_a_cue_emits_only_the_setting() {
    let mut model = Model::default();

    let cmd = step(
        &mut model,
        SettingRow::Appearance(AppearanceField::KeyHints),
        Direction::Next,
    );

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![&Effect::Config(ConfigCmd::SetAppearance(AppearancePatch {
            key_hints: Some(KeyHints::Hidden),
            ..AppearancePatch::default()
        }))]
    );
}

#[test]
fn stepping_a_row_changes_the_appearance_at_once() {
    let mut model = Model::default();

    drop(step(
        &mut model,
        SettingRow::Appearance(AppearanceField::KeyHints),
        Direction::Next,
    ));

    assert_eq!(
        model.settings.appearance_settings.key_hints,
        KeyHints::Hidden
    );
    assert_eq!(
        model.settings.appearance_settings.cover_mode,
        CoverMode::Vinyl
    );
}

#[test]
fn control_reads_the_appearance_row_of_its_field() {
    let count = OptionCount::new(4).unwrap();

    assert_eq!(
        SettingRow::Appearance(AppearanceField::CoverMode).control(),
        Some(AppearanceControl::Cycle(count))
    );
    assert_eq!(
        SettingRow::Appearance(AppearanceField::Animations).control(),
        Some(AppearanceControl::Toggle)
    );
    assert_eq!(SettingRow::Theme.control(), None);
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

struct PresetCase {
    preset: AppearancePreset,
    theme: Option<&'static str>,
}

#[rstest]
#[case::custom_steps_to_default(
    edited_appearance(),
    Direction::Next,
    PresetCase { preset: AppearancePreset::Stock, theme: None }
)]
#[case::default_steps_to_noir(
    AppearanceSettings::default(),
    Direction::Next,
    PresetCase { preset: AppearancePreset::Noir, theme: Some("noir") }
)]
#[case::noir_steps_to_default(
    preset_appearance(AppearancePreset::Noir),
    Direction::Previous,
    PresetCase { preset: AppearancePreset::Stock, theme: None }
)]
fn stepping_the_preset_row_selects_its_options_theme(
    #[case] appearance_settings: AppearanceSettings,
    #[case] direction: Direction,
    #[case] expected: PresetCase,
) {
    let field = AppearanceField::Preset;
    let mut model = Model::default();
    model.settings.appearance_settings = appearance_settings;

    let cmd = step(&mut model, SettingRow::Appearance(field), direction);

    assert_eq!(
        preset_of(model.settings.appearance_settings),
        Some(expected.preset)
    );
    let theme = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
            return None;
        };
        Some(choice.to_string())
    });
    assert_eq!(theme, expected.theme.map(str::to_string));
}

#[test]
fn stepping_the_preset_to_noir_selects_and_saves_the_noir_theme() {
    let theme_name = ThemeName::from_static("noir");
    let mut model = Model::default();

    let cmd = step(
        &mut model,
        SettingRow::Appearance(AppearanceField::Preset),
        Direction::Next,
    );

    assert_eq!(
        model.themes.theme_choice,
        ThemeChoice::Named(theme_name.clone())
    );
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(
        effects.contains(&&Effect::Config(ConfigCmd::Save(ConfigPatch {
            theme_name: Some(theme_name.clone()),
            ..ConfigPatch::default()
        }))),
        "{effects:?}"
    );
    assert!(
        effects.contains(&&Effect::Config(ConfigCmd::SelectTheme(
            ThemeChoice::Named(theme_name)
        ))),
        "{effects:?}"
    );
}
