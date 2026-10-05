use kernel::{
    cmd::{Cmd, ConfigCmd, Effect},
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
        appearance_rows::appearance_rows,
        cue::Cue,
        direction::Direction,
        model::Model,
        setting_row::{
            AppearanceControl,
            AppearanceField,
            AppearanceRow,
            AppearanceSetting,
            Choice,
            OptionCount,
            SettingRow,
        },
        time::Moment,
    },
    message::Message,
};
use rstest::rstest;

use crate::support::update::update;

fn step(model: &mut Model, row: SettingRow, direction: Direction) -> Cmd {
    update(model, Message::Step { row, direction }, Moment::default()).unwrap()
}

fn chosen(model: &Model, id: AppearanceField) -> Option<usize> {
    let slot = appearance_rows(model.settings.appearance)
        .into_iter()
        .find(|slot| slot.row.field == id)?;
    let Choice::Option(option) = slot.choice else {
        return None;
    };
    Some(option.get())
}

fn leaked(custom: AppearanceRow) -> &'static AppearanceRow {
    Box::leak(Box::new(custom))
}

fn custom_setting(
    field: AppearanceField,
    control: AppearanceControl,
) -> AppearanceSetting {
    AppearanceSetting {
        row: leaked(AppearanceRow {
            field,
            control,
            cue: None,
            themes: &[],
        }),
        choice: Choice::Option(control.count().index(0).unwrap()),
    }
}

#[rstest]
#[case::up_then_up_wraps(vec![Direction::Next, Direction::Next], vec![1, 0])]
#[case::down_wraps_backward(vec![Direction::Previous], vec![1])]
fn a_toggle_row_wraps_mod_two(
    #[case] steps: Vec<Direction>,
    #[case] expected_options: Vec<usize>,
) {
    let mut model = Model::default();
    let id = AppearanceField::KeyHints;

    let seen: Vec<usize> = steps
        .into_iter()
        .map(|direction| {
            drop(step(&mut model, SettingRow::Appearance(id), direction));
            chosen(&model, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(seen, expected_options);
}

#[test]
fn a_cycle_row_wraps_at_its_own_ring_size() {
    let mut model = Model::default();
    let id = AppearanceField::LayoutMode;

    let walked: Vec<usize> = (0..4)
        .map(|_| {
            drop(step(
                &mut model,
                SettingRow::Appearance(id),
                Direction::Next,
            ));
            chosen(&model, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(walked, vec![1, 2, 0, 1]);
}

#[test]
fn all_places_the_leading_custom_row_before_theme_then_the_rest_after() {
    let custom = vec![
        custom_setting(AppearanceField::CoverBrackets, AppearanceControl::Toggle),
        custom_setting(AppearanceField::CoverMode, AppearanceControl::Toggle),
        custom_setting(AppearanceField::SpeedChip, AppearanceControl::Toggle),
    ];

    let all = SettingRow::all(&custom);

    assert_eq!(
        all,
        vec![
            SettingRow::Appearance(AppearanceField::CoverBrackets),
            SettingRow::Theme,
            SettingRow::Appearance(AppearanceField::CoverMode),
            SettingRow::Appearance(AppearanceField::SpeedChip),
            SettingRow::Crossfade,
            SettingRow::ReplayGain,
            SettingRow::OutputDevice,
            SettingRow::SleepPresets,
        ]
    );
}

#[rstest]
#[case::toggle(AppearanceField::ProgressRemaining, 2)]
#[case::cycle(AppearanceField::LayoutMode, 3)]
#[case::four_options(AppearanceField::CoverMode, 4)]
fn stepping_every_option_of_a_row_never_reorders_settings_row_all(
    #[case] id: AppearanceField,
    #[case] option_count: usize,
) {
    let mut model = Model::default();
    let before = SettingRow::all(&appearance_rows(model.settings.appearance));

    for _ in 0..option_count {
        drop(step(
            &mut model,
            SettingRow::Appearance(id),
            Direction::Next,
        ));
        assert_eq!(
            SettingRow::all(&appearance_rows(model.settings.appearance)),
            before
        );
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
            &Effect::Config(ConfigCmd::SetAppearance(
                AppearancePatch::builder()
                    .layout_mode(LayoutMode::Full)
                    .build()
            )),
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
        vec![&Effect::Config(ConfigCmd::SetAppearance(
            AppearancePatch::builder()
                .key_hints(KeyHints::Hidden)
                .build()
        ))]
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

    assert_eq!(model.settings.appearance.key_hints, KeyHints::Hidden);
    assert_eq!(model.settings.appearance.cover_mode, CoverMode::Vinyl);
}

#[test]
fn control_reads_the_matching_slot_for_a_custom_row() {
    let count = OptionCount::new(5).unwrap();
    let custom = vec![custom_setting(
        AppearanceField::CoverMode,
        AppearanceControl::Cycle(count),
    )];

    assert_eq!(
        SettingRow::Appearance(AppearanceField::CoverMode).control(&custom),
        Some(AppearanceControl::Cycle(count))
    );
    assert_eq!(
        SettingRow::Appearance(AppearanceField::Animations).control(&custom),
        None
    );
    assert!(SettingRow::ReplayGain.activates(&custom));
    assert!(!SettingRow::Crossfade.activates(&custom));
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
    #[case] start: AppearanceSettings,
    #[case] direction: Direction,
    #[case] expected: (usize, Option<&str>),
) {
    let (expected_option, expected_theme) = expected;
    let id = AppearanceField::Preset;
    let mut model = Model::default();
    model.settings.appearance = start;

    let cmd = step(&mut model, SettingRow::Appearance(id), direction);

    assert_eq!(chosen(&model, id), Some(expected_option));
    let theme = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
            return None;
        };
        Some(choice.to_string())
    });
    assert_eq!(theme, expected_theme.map(str::to_string));
}
