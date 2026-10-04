use kernel::{
    cmd::{Cmd, ConfigCmd, Effect},
    domain::{
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
        theme::ThemeName,
        time::Moment,
    },
    message::Message,
};
use rstest::rstest;

use crate::support::update::update;

fn step(model: &mut Model, row: SettingRow, direction: Direction) -> Cmd {
    update(model, Message::Step { row, direction }, Moment::default()).unwrap()
}

fn setting_option(cmd: &Cmd, id: AppearanceField) -> Option<usize> {
    cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SetAppearance {
            field: emitted,
            option,
        }) = effect
        else {
            return None;
        };
        (*emitted == id).then_some(option.get())
    })
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

fn model_with_custom_row(control: AppearanceControl) -> (Model, AppearanceField) {
    let id = AppearanceField::KeyHints;
    let mut model = Model::default();
    model.appearance_rows.push(custom_setting(id, control));
    (model, id)
}

#[test]
fn stepping_an_unregistered_custom_row_emits_nothing() {
    let mut model = Model::default();

    let cmd = step(
        &mut model,
        SettingRow::Appearance(AppearanceField::Animations),
        Direction::Next,
    );

    assert!(cmd == Cmd::none());
}

#[rstest]
#[case::up_then_up_wraps(vec![Direction::Next, Direction::Next], vec![1, 0])]
#[case::down_wraps_backward(vec![Direction::Previous], vec![1])]
fn a_toggle_custom_row_wraps_mod_two(
    #[case] steps: Vec<Direction>,
    #[case] expected_options: Vec<usize>,
) {
    let (mut model, id) = model_with_custom_row(AppearanceControl::Toggle);

    let seen: Vec<usize> = steps
        .into_iter()
        .map(|direction| {
            let cmd = step(&mut model, SettingRow::Appearance(id), direction);
            setting_option(&cmd, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(seen, expected_options);
}

#[test]
fn a_cycle_custom_row_wraps_at_its_own_ring_size() {
    let (mut model, id) =
        model_with_custom_row(AppearanceControl::Cycle(OptionCount::new(3).unwrap()));

    let walked: Vec<usize> = (0..4)
        .map(|_| {
            let cmd = step(&mut model, SettingRow::Appearance(id), Direction::Next);
            setting_option(&cmd, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(walked, vec![1, 2, 0, 1]);
}

#[test]
fn a_step_custom_row_saturates_at_both_ends() {
    let (mut model, id) =
        model_with_custom_row(AppearanceControl::Step(OptionCount::new(3).unwrap()));

    let up = step(&mut model, SettingRow::Appearance(id), Direction::Next);
    assert_eq!(setting_option(&up, id), Some(1));
    let up_again = step(&mut model, SettingRow::Appearance(id), Direction::Next);
    assert_eq!(setting_option(&up_again, id), Some(2));
    let up_at_the_ceiling =
        step(&mut model, SettingRow::Appearance(id), Direction::Next);
    assert_eq!(setting_option(&up_at_the_ceiling, id), Some(2));

    let down = step(&mut model, SettingRow::Appearance(id), Direction::Previous);
    assert_eq!(setting_option(&down, id), Some(1));
    let down_to_floor =
        step(&mut model, SettingRow::Appearance(id), Direction::Previous);
    assert_eq!(setting_option(&down_to_floor, id), Some(0));
    let floored = step(&mut model, SettingRow::Appearance(id), Direction::Previous);
    assert_eq!(setting_option(&floored, id), Some(0));
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
#[case::toggle(AppearanceControl::Toggle, 2)]
#[case::cycle(AppearanceControl::Cycle(OptionCount::new(3).unwrap()), 3)]
#[case::step(AppearanceControl::Step(OptionCount::new(5).unwrap()), 5)]
fn stepping_every_option_of_a_custom_row_never_reorders_settings_row_all(
    #[case] control: AppearanceControl,
    #[case] option_count: usize,
) {
    let mut model = Model::default();
    model
        .appearance_rows
        .push(custom_setting(AppearanceField::ProgressRemaining, control));
    model.appearance_rows.push(custom_setting(
        AppearanceField::FormatChips,
        AppearanceControl::Toggle,
    ));

    let before = SettingRow::all(&model.appearance_rows);

    for _ in 0..option_count {
        drop(step(
            &mut model,
            SettingRow::Appearance(AppearanceField::ProgressRemaining),
            Direction::Next,
        ));
        assert_eq!(SettingRow::all(&model.appearance_rows), before);
    }
}

#[test]
fn stepping_a_custom_row_with_a_cue_emits_the_setting_then_the_cue() {
    let id = AppearanceField::LayoutMode;
    let count = OptionCount::new(3).unwrap();
    let mut model = Model::default();
    model.appearance_rows.push(AppearanceSetting {
        row: leaked(AppearanceRow {
            field: id,
            control: AppearanceControl::Cycle(count),
            cue: Some(Cue::LayoutChanged),
            themes: &[],
        }),
        choice: Choice::Option(count.index(0).unwrap()),
    });

    let cmd = step(&mut model, SettingRow::Appearance(id), Direction::Next);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![
            &Effect::Config(ConfigCmd::SetAppearance {
                field: id,
                option: count.index(1).unwrap(),
            }),
            &Effect::Animate(Cue::LayoutChanged),
        ]
    );
}

#[test]
fn stepping_a_custom_row_without_a_cue_emits_only_the_setting() {
    let (mut model, id) = model_with_custom_row(AppearanceControl::Toggle);

    let cmd = step(&mut model, SettingRow::Appearance(id), Direction::Next);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![&Effect::Config(ConfigCmd::SetAppearance {
            field: id,
            option: AppearanceControl::Toggle.count().index(1).unwrap(),
        })]
    );
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

#[rstest]
#[case::custom_steps_to_default(Choice::Mixed, Direction::Next, (0, None))]
#[case::default_steps_to_noir(Choice::Option(OptionCount::new(2).unwrap().index(0).unwrap()), Direction::Next, (1, Some("noir")))]
#[case::noir_steps_to_default(Choice::Option(OptionCount::new(2).unwrap().index(1).unwrap()), Direction::Previous, (0, None))]
fn stepping_the_preset_row_selects_its_options_theme(
    #[case] start: Choice,
    #[case] direction: Direction,
    #[case] expected: (usize, Option<&str>),
) {
    let themes: &'static [Option<ThemeName>] =
        Box::leak(Box::new([None, Some(ThemeName::from_static("noir"))]));

    let (expected_option, expected_theme) = expected;
    let id = AppearanceField::Preset;
    let count = OptionCount::new(2).unwrap();
    let mut model = Model::default();
    model.appearance_rows.push(AppearanceSetting {
        row: leaked(AppearanceRow {
            field: id,
            control: AppearanceControl::Cycle(count),
            cue: None,
            themes,
        }),
        choice: start,
    });

    let cmd = step(&mut model, SettingRow::Appearance(id), direction);

    assert_eq!(setting_option(&cmd, id), Some(expected_option));
    let theme = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
            return None;
        };
        Some(choice.to_string())
    });
    assert_eq!(theme, expected_theme.map(str::to_string));
}
