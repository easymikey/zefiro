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
        },
        appearance_rows::appearance_row_choices,
        cue::Cue,
        direction::Direction,
        model::Model,
        setting_row::{
            AppearanceControl,
            AppearanceField,
            Choice,
            OptionCount,
            SettingRow,
        },
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

fn chosen(model: &Model, field: AppearanceField) -> Option<usize> {
    let appearance_row_choice =
        appearance_row_choices(model.settings.appearance_settings)
            .into_iter()
            .find(|appearance_row_choice| appearance_row_choice.row.field == field)?;
    let Choice::Option(option) = appearance_row_choice.choice else {
        return None;
    };
    Some(option.get())
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
            chosen(&model, field).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(seen, expected_options);
}

#[test]
fn a_cycle_row_wraps_at_its_own_ring_size() {
    let mut model = Model::default();
    let field = AppearanceField::LayoutMode;

    let walked: Vec<usize> = (0..4)
        .map(|_| {
            drop(step(
                &mut model,
                SettingRow::Appearance(field),
                Direction::Next,
            ));
            chosen(&model, field).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(walked, vec![1, 2, 0, 1]);
}

#[test]
fn all_places_the_leading_custom_row_before_theme_then_the_rest_after() {
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
#[case::cycle(AppearanceField::LayoutMode, 3)]
#[case::four_options(AppearanceField::CoverMode, 4)]
fn stepping_every_option_of_a_row_never_reorders_settings_row_all(
    #[case] field: AppearanceField,
    #[case] option_count: usize,
) {
    let mut model = Model::default();
    let before = SettingRow::ALL;

    for _ in 0..option_count {
        drop(step(
            &mut model,
            SettingRow::Appearance(field),
            Direction::Next,
        ));
        assert_eq!(SettingRow::ALL, before);
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
                layout_mode: Some(LayoutMode::Full),
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

#[rstest]
#[case::custom_steps_to_default(edited_appearance(), Direction::Next, (0, None))]
#[case::default_steps_to_noir(AppearanceSettings::default(), Direction::Next, (1, Some("noir")))]
#[case::noir_steps_to_default(preset_appearance(AppearancePreset::Noir), Direction::Previous, (0, None))]
fn stepping_the_preset_row_selects_its_options_theme(
    #[case] appearance_settings: AppearanceSettings,
    #[case] direction: Direction,
    #[case] expected: (usize, Option<&str>),
) {
    let (expected_option, expected_theme) = expected;
    let field = AppearanceField::Preset;
    let mut model = Model::default();
    model.settings.appearance_settings = appearance_settings;

    let cmd = step(&mut model, SettingRow::Appearance(field), direction);

    assert_eq!(chosen(&model, field), Some(expected_option));
    let theme = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
            return None;
        };
        Some(choice.to_string())
    });
    assert_eq!(theme, expected_theme.map(str::to_string));
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
