use kernel::{
    Cmd,
    ConfigCmd,
    Cue,
    Direction,
    Effect,
    Message,
    Model,
    Moment,
    domain::{
        Choice,
        CustomControl,
        CustomRow,
        CustomSetting,
        OptionCount,
        SettingControl,
        SettingId,
        SettingRow,
        ThemeName,
    },
    update::update,
};
use rstest::rstest;

fn step(model: &mut Model, row: SettingRow, direction: Direction) -> Cmd {
    update(model, Message::Adjust { row, direction }, Moment::default()).unwrap()
}

fn setting_option(cmd: &Cmd, id: SettingId) -> Option<usize> {
    cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::Setting {
            id: emitted,
            option,
        }) = effect
        else {
            return None;
        };
        (*emitted == id).then_some(option.get())
    })
}

fn leaked(custom: CustomRow) -> &'static CustomRow {
    Box::leak(Box::new(custom))
}

fn custom_setting(id: SettingId, control: CustomControl) -> CustomSetting {
    CustomSetting {
        custom: leaked(CustomRow {
            id,
            control,
            cue: None,
            themes: &[],
        }),
        choice: Choice::Option(control.count().index(0).unwrap()),
    }
}

fn model_with_custom_row(control: CustomControl) -> (Model, SettingId) {
    let id = SettingId::new(7);
    let mut model = Model::default();
    model.custom_settings.push(custom_setting(id, control));
    (model, id)
}

#[test]
fn nudging_an_unregistered_custom_row_emits_nothing() {
    let mut model = Model::default();

    let cmd = step(
        &mut model,
        SettingRow::Custom(SettingId::new(9)),
        Direction::Next,
    );

    assert!(matches!(cmd, Cmd::None));
}

#[rstest]
#[case::up_then_up_wraps(vec![Direction::Next, Direction::Next], vec![1, 0])]
#[case::down_wraps_backward(vec![Direction::Previous], vec![1])]
fn a_toggle_custom_row_wraps_mod_two(
    #[case] nudges: Vec<Direction>,
    #[case] expected_options: Vec<usize>,
) {
    let (mut model, id) = model_with_custom_row(CustomControl::Toggle);

    let seen: Vec<usize> = nudges
        .into_iter()
        .map(|direction| {
            let cmd = step(&mut model, SettingRow::Custom(id), direction);
            setting_option(&cmd, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(seen, expected_options);
}

#[test]
fn a_cycle_custom_row_wraps_at_its_own_ring_size() {
    let (mut model, id) =
        model_with_custom_row(CustomControl::Cycle(OptionCount::new(3).unwrap()));

    let walked: Vec<usize> = (0..4)
        .map(|_| {
            let cmd = step(&mut model, SettingRow::Custom(id), Direction::Next);
            setting_option(&cmd, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(walked, vec![1, 2, 0, 1]);
}

#[test]
fn a_step_custom_row_saturates_at_both_ends() {
    let (mut model, id) =
        model_with_custom_row(CustomControl::Step(OptionCount::new(3).unwrap()));

    let up = step(&mut model, SettingRow::Custom(id), Direction::Next);
    assert_eq!(setting_option(&up, id), Some(1));
    let up_again = step(&mut model, SettingRow::Custom(id), Direction::Next);
    assert_eq!(setting_option(&up_again, id), Some(2));
    let up_at_the_ceiling = step(&mut model, SettingRow::Custom(id), Direction::Next);
    assert_eq!(setting_option(&up_at_the_ceiling, id), Some(2));

    let down = step(&mut model, SettingRow::Custom(id), Direction::Previous);
    assert_eq!(setting_option(&down, id), Some(1));
    let down_to_floor = step(&mut model, SettingRow::Custom(id), Direction::Previous);
    assert_eq!(setting_option(&down_to_floor, id), Some(0));
    let floored = step(&mut model, SettingRow::Custom(id), Direction::Previous);
    assert_eq!(setting_option(&floored, id), Some(0));
}

#[test]
fn all_places_the_leading_custom_row_before_theme_then_the_rest_after() {
    let custom = vec![
        custom_setting(SettingId::new(1), CustomControl::Toggle),
        custom_setting(SettingId::new(2), CustomControl::Toggle),
        custom_setting(SettingId::new(3), CustomControl::Toggle),
    ];

    let all = SettingRow::all(&custom);

    assert_eq!(
        all,
        vec![
            SettingRow::Custom(SettingId::new(1)),
            SettingRow::Theme,
            SettingRow::Custom(SettingId::new(2)),
            SettingRow::Custom(SettingId::new(3)),
            SettingRow::Crossfade,
            SettingRow::Replaygain,
            SettingRow::OutputDevice,
            SettingRow::SleepPresets,
        ]
    );
}

#[rstest]
#[case::toggle(CustomControl::Toggle, 2)]
#[case::cycle(CustomControl::Cycle(OptionCount::new(3).unwrap()), 3)]
#[case::step(CustomControl::Step(OptionCount::new(5).unwrap()), 5)]
fn nudging_every_option_of_a_custom_row_never_reorders_settings_row_all(
    #[case] control: CustomControl,
    #[case] options: usize,
) {
    let mut model = Model::default();
    model
        .custom_settings
        .push(custom_setting(SettingId::new(5), control));
    model
        .custom_settings
        .push(custom_setting(SettingId::new(6), CustomControl::Toggle));

    let before = SettingRow::all(&model.custom_settings);

    for _ in 0..options {
        let _ = step(
            &mut model,
            SettingRow::Custom(SettingId::new(5)),
            Direction::Next,
        );
        assert_eq!(SettingRow::all(&model.custom_settings), before);
    }
}

#[test]
fn nudging_a_custom_row_with_a_cue_emits_the_setting_then_the_cue() {
    let id = SettingId::new(11);
    let count = OptionCount::new(3).unwrap();
    let mut model = Model::default();
    model.custom_settings.push(CustomSetting {
        custom: leaked(CustomRow {
            id,
            control: CustomControl::Cycle(count),
            cue: Some(Cue::LayoutChanged),
            themes: &[],
        }),
        choice: Choice::Option(count.index(0).unwrap()),
    });

    let cmd = step(&mut model, SettingRow::Custom(id), Direction::Next);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![
            &Effect::Config(ConfigCmd::Setting {
                id,
                option: count.index(1).unwrap(),
            }),
            &Effect::Animate(Cue::LayoutChanged),
        ]
    );
}

#[test]
fn nudging_a_custom_row_without_a_cue_emits_only_the_setting() {
    let (mut model, id) = model_with_custom_row(CustomControl::Toggle);

    let cmd = step(&mut model, SettingRow::Custom(id), Direction::Next);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![&Effect::Config(ConfigCmd::Setting {
            id,
            option: CustomControl::Toggle.count().index(1).unwrap(),
        })]
    );
}

#[test]
fn control_reads_the_matching_slot_for_a_custom_row() {
    let count = OptionCount::new(5).unwrap();
    let custom = vec![custom_setting(
        SettingId::new(4),
        CustomControl::Cycle(count),
    )];

    assert_eq!(
        SettingRow::Custom(SettingId::new(4)).control(&custom),
        Some(SettingControl::Custom(CustomControl::Cycle(count)))
    );
    assert_eq!(
        SettingRow::Custom(SettingId::new(404)).control(&custom),
        None
    );
    assert_eq!(
        SettingRow::Replaygain.control(&custom),
        Some(SettingControl::Toggle)
    );
}

#[rstest]
#[case::custom_nudges_to_default(Choice::Mixed, Direction::Next, (0, None))]
#[case::default_nudges_to_noir(Choice::Option(OptionCount::new(2).unwrap().index(0).unwrap()), Direction::Next, (1, Some("noir")))]
#[case::noir_nudges_to_default(Choice::Option(OptionCount::new(2).unwrap().index(1).unwrap()), Direction::Previous, (0, None))]
fn nudging_the_preset_row_selects_its_options_theme(
    #[case] start: Choice,
    #[case] direction: Direction,
    #[case] expected: (usize, Option<&str>),
) {
    let themes: &'static [Option<ThemeName>] =
        Box::leak(Box::new([None, Some(ThemeName::from_static("noir"))]));

    let (expected_option, expected_theme) = expected;
    let id = SettingId::new(200);
    let count = OptionCount::new(2).unwrap();
    let mut model = Model::default();
    model.custom_settings.push(CustomSetting {
        custom: leaked(CustomRow {
            id,
            control: CustomControl::Cycle(count),
            cue: None,
            themes,
        }),
        choice: start,
    });

    let cmd = step(&mut model, SettingRow::Custom(id), direction);

    assert_eq!(setting_option(&cmd, id), Some(expected_option));
    let theme = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
            return None;
        };
        Some(choice.to_string())
    });
    assert_eq!(theme, expected_theme.map(str::to_string));
}
