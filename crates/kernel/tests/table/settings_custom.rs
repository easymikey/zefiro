use kernel::{
    Cmd,
    ConfigCmd,
    Cue,
    Effect,
    Message,
    Model,
    Nudge,
    domain::{CustomSetting, SettingControl, SettingId, SettingRow},
    update::update,
};
use rstest::rstest;

fn step(model: &mut Model, row: SettingRow, nudge: Nudge) -> Cmd {
    update(model, Message::Adjust { row, nudge }).unwrap()
}

fn setting_position(cmd: &Cmd, id: SettingId) -> Option<usize> {
    cmd.effects().find_map(|effect| {
        let Effect::Setting {
            id: emitted,
            position,
        } = effect
        else {
            return None;
        };
        (*emitted == id).then_some(*position)
    })
}

fn model_with_custom_row(control: SettingControl) -> (Model, SettingId) {
    let id = SettingId(7);
    let mut model = Model::default();
    model.custom_rows.push(CustomSetting {
        id,
        control,
        position: 0,
        cue: None,
        themes: &[],
    });
    (model, id)
}

#[test]
fn nudging_an_unregistered_custom_row_emits_nothing() {
    let mut model = Model::default();

    let cmd = step(&mut model, SettingRow::Custom(SettingId(9)), Nudge::Up);

    assert!(matches!(cmd, Cmd::None));
}

#[rstest]
#[case::up_then_up_wraps(vec![Nudge::Up, Nudge::Up], vec![1, 0])]
#[case::down_wraps_backward(vec![Nudge::Down], vec![1])]
fn a_toggle_custom_row_wraps_mod_two(
    #[case] nudges: Vec<Nudge>,
    #[case] expected_positions: Vec<usize>,
) {
    let (mut model, id) = model_with_custom_row(SettingControl::Toggle);

    let seen: Vec<usize> = nudges
        .into_iter()
        .map(|nudge| {
            let cmd = step(&mut model, SettingRow::Custom(id), nudge);
            setting_position(&cmd, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(seen, expected_positions);
}

#[test]
fn a_cycle_custom_row_wraps_at_its_own_ring_size() {
    let (mut model, id) = model_with_custom_row(SettingControl::Cycle(3));

    let walked: Vec<usize> = (0..4)
        .map(|_| {
            let cmd = step(&mut model, SettingRow::Custom(id), Nudge::Up);
            setting_position(&cmd, id).unwrap_or(usize::MAX)
        })
        .collect();

    assert_eq!(walked, vec![1, 2, 0, 1]);
}

#[test]
fn a_step_custom_row_has_no_ceiling_and_floors_at_zero() {
    let (mut model, id) = model_with_custom_row(SettingControl::Step);

    let up = step(&mut model, SettingRow::Custom(id), Nudge::Up);
    assert_eq!(setting_position(&up, id), Some(1));
    let up_again = step(&mut model, SettingRow::Custom(id), Nudge::Up);
    assert_eq!(setting_position(&up_again, id), Some(2));

    let down = step(&mut model, SettingRow::Custom(id), Nudge::Down);
    assert_eq!(setting_position(&down, id), Some(1));
    let down_to_floor = step(&mut model, SettingRow::Custom(id), Nudge::Down);
    assert_eq!(setting_position(&down_to_floor, id), Some(0));
    let floored = step(&mut model, SettingRow::Custom(id), Nudge::Down);
    assert_eq!(setting_position(&floored, id), Some(0));
}

#[test]
fn all_places_the_leading_custom_row_before_theme_then_the_rest_after() {
    let custom = vec![
        CustomSetting {
            id: SettingId(1),
            control: SettingControl::Toggle,
            position: 2,
            cue: None,
            themes: &[],
        },
        CustomSetting {
            id: SettingId(2),
            control: SettingControl::Toggle,
            position: 0,
            cue: None,
            themes: &[],
        },
        CustomSetting {
            id: SettingId(3),
            control: SettingControl::Toggle,
            position: 1,
            cue: None,
            themes: &[],
        },
    ];

    let all = SettingRow::all(&custom);

    assert_eq!(
        all,
        vec![
            SettingRow::Custom(SettingId(1)),
            SettingRow::Theme,
            SettingRow::Custom(SettingId(2)),
            SettingRow::Custom(SettingId(3)),
            SettingRow::Crossfade,
            SettingRow::Replaygain,
            SettingRow::OutputDevice,
            SettingRow::SleepPresets,
        ]
    );
}

#[rstest]
#[case::toggle(SettingControl::Toggle, 2)]
#[case::cycle(SettingControl::Cycle(3), 3)]
#[case::step(SettingControl::Step, 5)]
fn nudging_every_option_of_a_custom_row_never_reorders_settings_row_all(
    #[case] control: SettingControl,
    #[case] options: usize,
) {
    let mut model = Model::default();
    model.custom_rows.push(CustomSetting {
        id: SettingId(5),
        control,
        position: 0,
        cue: None,
        themes: &[],
    });
    model.custom_rows.push(CustomSetting {
        id: SettingId(6),
        control: SettingControl::Toggle,
        position: 0,
        cue: None,
        themes: &[],
    });

    let before = SettingRow::all(&model.custom_rows);

    for _ in 0..options {
        let _ = step(&mut model, SettingRow::Custom(SettingId(5)), Nudge::Up);
        assert_eq!(SettingRow::all(&model.custom_rows), before);
    }
}

#[test]
fn nudging_a_custom_row_with_a_cue_emits_the_setting_then_the_cue() {
    let id = SettingId(11);
    let mut model = Model::default();
    model.custom_rows.push(CustomSetting {
        id,
        control: SettingControl::Cycle(3),
        position: 0,
        cue: Some(Cue::LayoutChanged),
        themes: &[],
    });

    let cmd = step(&mut model, SettingRow::Custom(id), Nudge::Up);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(
        effects,
        vec![
            &Effect::Setting { id, position: 1 },
            &Effect::Animate(Cue::LayoutChanged),
        ]
    );
}

#[test]
fn nudging_a_custom_row_without_a_cue_emits_only_the_setting() {
    let (mut model, id) = model_with_custom_row(SettingControl::Toggle);

    let cmd = step(&mut model, SettingRow::Custom(id), Nudge::Up);

    let effects: Vec<&Effect> = cmd.effects().collect();
    assert_eq!(effects, vec![&Effect::Setting { id, position: 1 }]);
}

#[test]
fn control_reads_the_matching_slot_for_a_custom_row() {
    let custom = vec![CustomSetting {
        id: SettingId(4),
        control: SettingControl::Cycle(5),
        position: 0,
        cue: None,
        themes: &[],
    }];

    assert_eq!(
        SettingRow::Custom(SettingId(4)).control(&custom),
        SettingControl::Cycle(5)
    );
    assert_eq!(
        SettingRow::Custom(SettingId(404)).control(&custom),
        SettingControl::Step
    );
    assert_eq!(
        SettingRow::Replaygain.control(&custom),
        SettingControl::Toggle
    );
}

#[rstest]
#[case::custom_nudges_to_default(2, Nudge::Up, (0, None))]
#[case::default_nudges_to_noir(0, Nudge::Up, (1, Some("noir")))]
#[case::noir_nudges_to_default(1, Nudge::Down, (0, None))]
fn nudging_the_preset_row_selects_its_options_theme(
    #[case] start: usize,
    #[case] nudge: Nudge,
    #[case] expected: (usize, Option<&str>),
) {
    let (expected_position, expected_theme) = expected;
    let id = SettingId(200);
    let mut model = Model::default();
    model.custom_rows.push(CustomSetting {
        id,
        control: SettingControl::Cycle(2),
        position: start,
        cue: None,
        themes: &[None, Some("noir")],
    });

    let cmd = step(&mut model, SettingRow::Custom(id), nudge);

    assert_eq!(setting_position(&cmd, id), Some(expected_position));
    let theme = cmd.effects().find_map(|effect| {
        let Effect::Config(ConfigCmd::SelectTheme(name)) = effect else {
            return None;
        };
        Some(name.as_str())
    });
    assert_eq!(theme, expected_theme);
}
