use std::time::Duration;

use kernel::{
    cmd::{AudioCmd, Cmd, ConfigCmd, Effect},
    domain::{
        appearance::AppearanceSettings,
        appearance_rows::appearance_rows,
        crossfade::Crossfade,
        device::{DeviceDefault, ListedDevice, OutputDevice},
        direction::Direction,
        model::Model,
        overlay::{Overlay, OverlayName},
        setting_row::{AppearanceField, SettingRow},
        settings::ReplayGain,
        sleep_presets::SleepPresets,
        theme::{ThemeChoice, ThemeName, Themes},
        time::Moment,
    },
    message::{
        ConfigEvent,
        Message,
        OverlayRequest,
        PlaybackRequest,
        SettingsRowRequest,
    },
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    device,
    update::{send, update},
};

fn all_rows() -> Vec<SettingRow> {
    SettingRow::all(&appearance_rows(AppearanceSettings::default()))
}

fn row_index(row: SettingRow) -> usize {
    let found = all_rows().iter().position(|candidate| *candidate == row);
    found.unwrap_or_else(|| panic!("row missing from the settings list: {row:?}"))
}

fn navigate(model: &mut Model, direction: Direction) -> Cmd {
    let request = SettingsRowRequest::Navigate(direction);
    let message = Message::Overlay(OverlayRequest::Settings(request));
    update(model, message, Moment::default()).unwrap()
}

fn opened_settings() -> Model {
    let mut model = Model::default();
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::Settings)),
    );
    model
}

fn navigated_to(index: usize) -> Model {
    let mut model = opened_settings();
    for _ in 0..index {
        navigate_down(&mut model);
    }
    model
}

#[test]
fn navigate_down_steps_to_the_next_row() {
    let mut model = navigated_to(0);
    let cmd = navigate(&mut model, Direction::Next);
    assert_eq!(selected_row(&model), all_rows().get(1).copied());
    assert!(cmd == Cmd::none());
}

#[test]
fn navigate_up_clamps_at_the_first_row() {
    let mut model = navigated_to(0);
    let cmd = navigate(&mut model, Direction::Previous);
    assert_eq!(selected_row(&model), all_rows().first().copied());
    assert!(cmd == Cmd::none());
}

#[test]
fn navigate_down_clamps_at_the_last_row() {
    let last = all_rows().len() - 1;
    let mut model = navigated_to(last);
    let cmd = navigate(&mut model, Direction::Next);
    assert_eq!(selected_row(&model), all_rows().last().copied());
    assert!(cmd == Cmd::none());
}

#[rstest]
#[case::step_hands_the_router_the_selected_row(SettingRow::ReplayGain, Direction::Next)]
#[case::step_keeps_the_direction(SettingRow::Crossfade, Direction::Previous)]
fn step_resolves_the_row_under_the_cursor(
    #[case] row: SettingRow,
    #[case] direction: Direction,
) {
    let mut model = navigated_to(row_index(row));
    assert_eq!(selected_row(&model), Some(row));

    let request = SettingsRowRequest::Step(direction);
    let cmd = update(
        &mut model,
        Message::Overlay(OverlayRequest::Settings(request)),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(selected_row(&model), Some(row));
    assert!(cmd != Cmd::none());
}

fn seeded() -> Model {
    let mut model = Model {
        themes: Themes {
            names: vec![
                ThemeName::from_static("noir"),
                ThemeName::from_static("solar"),
                ThemeName::from_static("mono"),
            ],
            selected: ThemeChoice::Named(ThemeName::from_static("noir")),
        },
        ..Model::default()
    };
    model.settings.output_devices = vec![
        ListedDevice {
            name: device("Speakers"),
            default: DeviceDefault::Default,
        },
        ListedDevice {
            name: device("Headphones"),
            default: DeviceDefault::Named,
        },
    ];
    model
}

#[test]
fn step_row_theme_never_touches_window_colors() {
    let mut model = seeded();

    let cmd = step(&mut model, SettingRow::Theme, Direction::Next);

    assert!(
        !cmd.effects()
            .any(|effect| matches!(effect, Effect::WindowColors(_)))
    );
}

fn press(model: &mut Model, row: SettingRow, direction: Direction) {
    send(model, Message::Step { row, direction });
}

fn step(model: &mut Model, row: SettingRow, direction: Direction) -> Cmd {
    update(model, Message::Step { row, direction }, Moment::default()).unwrap()
}

#[test]
fn step_row_toggles_a_config_row() {
    fn saved(cmd: &Cmd) -> bool {
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Config(ConfigCmd::Save(_))))
    }
    fn live_effects(cmd: &Cmd) -> Vec<String> {
        cmd.effects()
            .filter_map(|effect| match effect {
                Effect::Audio(command) => Some(format!("{command:?}")),
                Effect::Library(command) => Some(format!("{command:?}")),
                Effect::RollShuffle(..) => Some("RollShuffle".to_string()),
                Effect::WindowColors(_)
                | Effect::Config(_)
                | Effect::Macos(_)
                | Effect::Animate(_)
                | Effect::After { .. }
                | Effect::Restart(_)
                | Effect::Quit => None,
            })
            .collect()
    }

    let mut model = seeded();
    let cmd = step(&mut model, SettingRow::ReplayGain, Direction::Next);

    assert_eq!(model.settings.audio.replay_gain, ReplayGain::On);
    assert!(saved(&cmd));
    assert!(
        live_effects(&cmd)
            .iter()
            .any(|effect| effect == "SetReplayGain(On)")
    );

    let toggled_back = step(&mut model, SettingRow::ReplayGain, Direction::Previous);
    assert_eq!(model.settings.audio.replay_gain, ReplayGain::Off);
    assert!(saved(&toggled_back));
}

#[test]
fn step_row_crossfade_steps_by_500ms_and_clamps_both_ends() {
    fn crossfade_patch(cmd: &Cmd) -> Option<Crossfade> {
        cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::Save(patch)) = effect else {
                return None;
            };
            patch.crossfade
        })
    }

    let mut model = seeded();
    let half_second = Crossfade::try_from(Duration::from_millis(500)).unwrap();

    let cmd = step(&mut model, SettingRow::Crossfade, Direction::Previous);
    assert_eq!(model.settings.audio.crossfade, Crossfade::default());
    assert_eq!(crossfade_patch(&cmd), Some(Crossfade::default()));

    let stepped_up = step(&mut model, SettingRow::Crossfade, Direction::Next);
    assert_eq!(model.settings.audio.crossfade, half_second);
    assert!(
        stepped_up
            .effects()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::SetCrossfade(_))))
    );

    for _ in 0..21 {
        press(&mut model, SettingRow::Crossfade, Direction::Next);
    }
    let ceiling = Crossfade::try_from(Duration::from_secs(10)).unwrap();
    assert_eq!(model.settings.audio.crossfade, ceiling);
}

#[test]
fn step_row_theme_selects_the_theme_and_raises_no_wash_cue() {
    let mut model = seeded();

    let cmd = step(&mut model, SettingRow::Theme, Direction::Next);

    assert!(cmd.effects().any(|effect| matches!(
        effect,
        Effect::Config(ConfigCmd::SelectTheme(choice))
            if choice.to_string() == "solar"
    )));
    assert!(
        !cmd.effects()
            .any(|effect| matches!(effect, Effect::Animate(_)))
    );
}

#[rstest]
#[case::forward_one(Direction::Next, &["solar", "mono", "noir"])]
#[case::backward_one(Direction::Previous, &["mono", "solar", "noir"])]
fn step_row_theme_cycles_model_themes_and_wraps(
    #[case] direction: Direction,
    #[case] walk: &[&str],
) {
    fn theme_patch(cmd: &Cmd) -> Option<String> {
        cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::Save(patch)) = effect else {
                return None;
            };
            patch.theme.as_ref().map(ThemeName::to_string)
        })
    }

    let mut model = seeded();
    for expected in walk {
        let cmd = step(&mut model, SettingRow::Theme, direction);
        assert_eq!(model.themes.selected.to_string(), *expected);
        assert_eq!(theme_patch(&cmd).as_deref(), Some(*expected));
    }
}

#[rstest]
#[case::theme(SettingRow::Theme, |model: &Model| model.themes.selected == ThemeChoice::Auto)]
#[case::output_device(SettingRow::OutputDevice, |model: &Model| model
    .settings
    .audio
    .device
    == OutputDevice::SystemDefault)]
fn step_row_does_nothing_until_the_shell_delivers_a_list(
    #[case] row: SettingRow,
    #[case] unchanged: fn(&Model) -> bool,
) {
    let mut model = Model::default();

    let result = update(
        &mut model,
        Message::Step {
            row,
            direction: Direction::Next,
        },
        Moment::default(),
    );

    assert!(unchanged(&model));
    assert_eq!(result, Err(Unhandled));
}

#[test]
fn step_row_output_device_cycles_system_default_and_devices_and_wraps() {
    fn device_patch(cmd: &Cmd) -> Option<OutputDevice> {
        cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::Save(patch)) = effect else {
                return None;
            };
            patch.device.clone()
        })
    }

    let mut model = seeded();
    assert_eq!(model.settings.audio.device, OutputDevice::SystemDefault);

    let cmd = step(&mut model, SettingRow::OutputDevice, Direction::Next);
    assert_eq!(
        model.settings.audio.device,
        OutputDevice::Named(device("Speakers"))
    );
    assert_eq!(
        device_patch(&cmd),
        Some(OutputDevice::Named(device("Speakers")))
    );
    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::SetDevice(_))))
    );

    press(&mut model, SettingRow::OutputDevice, Direction::Next);
    assert_eq!(
        model.settings.audio.device,
        OutputDevice::Named(device("Headphones"))
    );

    press(&mut model, SettingRow::OutputDevice, Direction::Next);
    assert_eq!(model.settings.audio.device, OutputDevice::SystemDefault);

    press(&mut model, SettingRow::OutputDevice, Direction::Previous);
    assert_eq!(
        model.settings.audio.device,
        OutputDevice::Named(device("Headphones"))
    );
}

#[test]
fn step_row_sleep_presets_cycles_and_wraps_and_persists() {
    fn sleep_presets_patch(cmd: &Cmd) -> Option<SleepPresets> {
        cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::Save(patch)) = effect else {
                return None;
            };
            patch.sleep_presets.clone()
        })
    }

    let mut model = seeded();
    assert_eq!(
        Some(model.settings.audio.sleep_presets.as_slice()),
        SleepPresets::BUNDLES.first().copied()
    );

    let cmd = step(&mut model, SettingRow::SleepPresets, Direction::Next);
    assert_eq!(
        Some(model.settings.audio.sleep_presets.as_slice()),
        SleepPresets::BUNDLES.get(1).copied()
    );
    assert_eq!(sleep_presets_patch(&cmd), SleepPresets::bundle(1));

    press(&mut model, SettingRow::SleepPresets, Direction::Previous);
    let wrapped = step(&mut model, SettingRow::SleepPresets, Direction::Previous);
    assert!(model.settings.audio.sleep_presets.as_slice().is_empty());
    assert_eq!(sleep_presets_patch(&wrapped), SleepPresets::bundle(4));
}

#[test]
fn step_row_sleep_presets_snaps_a_custom_value_to_the_nearest_bundle() {
    let mut model = seeded();
    model.settings.audio.sleep_presets = SleepPresets::from_minutes(&[100]).unwrap();

    press(&mut model, SettingRow::SleepPresets, Direction::Next);

    assert_eq!(
        Some(model.settings.audio.sleep_presets.as_slice()),
        SleepPresets::BUNDLES.get(1).copied()
    );
}

#[test]
fn step_row_sleep_presets_leaves_the_clamp_to_the_next_cycle() {
    let mut model = seeded();
    model.transport.sleep = Some(kernel::domain::sleep::SleepTimer {
        preset_index: kernel::domain::index::PresetIndex::new(2),
        delay: Duration::from_secs(60),
        deadline: Moment::new(Duration::from_secs(60)),
    });

    press(&mut model, SettingRow::SleepPresets, Direction::Previous);
    let armed = model.transport.sleep.map(|timer| timer.preset_index.get());
    send(&mut model, Message::Playback(PlaybackRequest::CycleSleep));

    assert_eq!(armed, Some(2));
    assert!(model.transport.sleep.is_none());
}

#[test]
fn step_row_keeps_the_two_config_files_apart() {
    let mut model = seeded();

    let audio = step(&mut model, SettingRow::ReplayGain, Direction::Next);
    let audio_effects: Vec<&Effect> = audio
        .effects()
        .filter(|effect| matches!(effect, Effect::Audio(_) | Effect::Library(_)))
        .collect();
    assert!(matches!(
        audio_effects.as_slice(),
        [Effect::Audio(AudioCmd::SetReplayGain(ReplayGain::On))]
    ));

    let custom = step(
        &mut model,
        SettingRow::Appearance(AppearanceField::LayoutMode),
        Direction::Next,
    );
    assert!(
        !custom
            .effects()
            .any(|effect| matches!(effect, Effect::Config(ConfigCmd::Save(_))))
    );
    assert!(
        !custom
            .effects()
            .any(|effect| matches!(effect, Effect::Audio(_) | Effect::Library(_)))
    );
    assert!(
        custom.effects().any(|effect| matches!(
            effect,
            Effect::Config(ConfigCmd::SetAppearance(_))
        ))
    );
}

fn selected_row(model: &Model) -> Option<SettingRow> {
    let Some(Overlay::Settings(selected)) = &model.workspace.overlay else {
        return None;
    };
    Some(*selected)
}

fn navigate_down(model: &mut Model) {
    drop(navigate(model, Direction::Next));
}

#[test]
fn the_highlighted_row_is_the_row_that_changes_across_steps() {
    let mut model = Model::default();
    let cover_mode = AppearanceField::CoverMode;

    send(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::Settings)),
    );
    navigate_down(&mut model);
    navigate_down(&mut model);
    assert_eq!(
        selected_row(&model),
        Some(SettingRow::Appearance(cover_mode))
    );

    for _ in 0..3 {
        let cmd = update(
            &mut model,
            Message::Overlay(OverlayRequest::Settings(SettingsRowRequest::Step(
                Direction::Next,
            ))),
            Moment::default(),
        )
        .unwrap();
        let patched_cover_mode = cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::SetAppearance(patch)) = effect else {
                return None;
            };
            Some(patch.cover_mode.is_some())
        });
        assert_eq!(patched_cover_mode, Some(true));
        assert_eq!(
            selected_row(&model),
            Some(SettingRow::Appearance(cover_mode))
        );
    }
}

#[test]
fn an_appearance_reload_while_open_keeps_the_selection_on_the_same_row() {
    let mut model = Model::default();
    let cover_mode = AppearanceField::CoverMode;

    send(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::Settings)),
    );
    navigate_down(&mut model);
    navigate_down(&mut model);
    assert_eq!(
        selected_row(&model),
        Some(SettingRow::Appearance(cover_mode))
    );

    send(
        &mut model,
        Message::Config(ConfigEvent::AppearanceReloaded(
            AppearanceSettings::default(),
        )),
    );

    assert_eq!(
        selected_row(&model),
        Some(SettingRow::Appearance(cover_mode))
    );
}
