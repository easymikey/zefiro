use std::time::Duration;

use kernel::{
    cmd::{AudioCmd, Cmd, ConfigCmd, Effect},
    domain::{
        appearance::{AppearanceSettings, KeyHints},
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
        SettingRowRequest,
    },
    update::{
        machine::{Machine, Unhandled},
        overlay::settings::SettingRowMessage,
    },
};
use rstest::rstest;

use crate::support::{
    device,
    update::{send, update},
};

fn navigate(model: &mut Model, direction: Direction) -> Cmd {
    let request = SettingRowRequest::Navigate(direction);
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

fn navigated_to(row_index: usize) -> Model {
    let mut model = opened_settings();
    for _ in 0..row_index {
        navigate_down(&mut model);
    }
    model
}

struct NavigateRow {
    start: usize,
    direction: Direction,
    answer: Result<Cmd, Unhandled>,
    lands_on: usize,
}

#[rstest]
#[case::down_steps_to_the_next_row(NavigateRow {
    start: 0,
    direction: Direction::Next,
    answer: Ok(Cmd::none()),
    lands_on: 1,
})]
#[case::up_from_the_first_row_is_refused(NavigateRow {
    start: 0,
    direction: Direction::Previous,
    answer: Err(Unhandled),
    lands_on: 0,
})]
#[case::down_from_the_last_row_is_refused(NavigateRow {
    start: SettingRow::ALL.len() - 1,
    direction: Direction::Next,
    answer: Err(Unhandled),
    lands_on: SettingRow::ALL.len() - 1,
})]
fn navigate_steps_the_selection_and_is_refused_past_either_end(
    #[case] row: NavigateRow,
) {
    let NavigateRow {
        start,
        direction,
        answer: expected,
        lands_on,
    } = row;
    let mut model = navigated_to(start);
    let request = SettingRowRequest::Navigate(direction);

    let answer = update(
        &mut model,
        Message::Overlay(OverlayRequest::Settings(request)),
        Moment::default(),
    );

    assert_eq!(answer, expected);
    assert_eq!(selected_row(&model), SettingRow::ALL.get(lands_on).copied());
}

#[test]
fn setting_the_selected_row_again_is_refused() {
    let mut setting_row = SettingRow::Crossfade;
    let result = setting_row.transition(SettingRowMessage::Set(SettingRow::Crossfade));
    assert_eq!(result, Err(Unhandled));
    assert_eq!(setting_row, SettingRow::Crossfade);
}

fn seeded() -> Model {
    let mut model = Model {
        themes: Themes {
            names: vec![
                ThemeName::from_static("noir"),
                ThemeName::from_static("solar"),
                ThemeName::from_static("mono"),
            ],
            theme_choice: ThemeChoice::Named(ThemeName::from_static("noir")),
        },
        ..Model::default()
    };
    model.settings.output_devices = vec![
        ListedDevice {
            name: device("Speakers"),
            default: DeviceDefault::Yes,
        },
        ListedDevice {
            name: device("Headphones"),
            default: DeviceDefault::No,
        },
    ];
    model
}

fn press(model: &mut Model, row: SettingRow, direction: Direction) {
    send(model, Message::Step { row, direction });
}

fn step(model: &mut Model, row: SettingRow, direction: Direction) -> Cmd {
    update(model, Message::Step { row, direction }, Moment::default()).unwrap()
}

#[test]
fn step_row_toggles_a_config_row_and_keeps_the_two_config_files_apart() {
    fn saves(cmd: &Cmd) -> bool {
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Config(ConfigCmd::Save(_))))
    }
    let mut model = seeded();
    let cmd = step(&mut model, SettingRow::ReplayGain, Direction::Next);

    assert_eq!(model.settings.audio_settings.replay_gain, ReplayGain::On);
    assert!(saves(&cmd));
    let audio_effects: Vec<&Effect> = cmd
        .effects()
        .filter(|effect| matches!(effect, Effect::Audio(_) | Effect::Library(_)))
        .collect();
    assert!(matches!(
        audio_effects.as_slice(),
        [Effect::Audio(AudioCmd::SetReplayGain(ReplayGain::On))]
    ));

    let toggled_back = step(&mut model, SettingRow::ReplayGain, Direction::Previous);
    assert_eq!(model.settings.audio_settings.replay_gain, ReplayGain::Off);
    assert!(saves(&toggled_back));

    let custom = step(
        &mut model,
        SettingRow::Appearance(AppearanceField::LayoutMode),
        Direction::Next,
    );
    assert!(!saves(&custom));
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

    let stepped_up = step(&mut model, SettingRow::Crossfade, Direction::Next);
    assert_eq!(model.settings.audio_settings.crossfade, half_second);
    assert_eq!(crossfade_patch(&stepped_up), Some(half_second));
    assert!(
        stepped_up
            .effects()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::SetCrossfade(_))))
    );

    for _ in 0..19 {
        press(&mut model, SettingRow::Crossfade, Direction::Next);
    }
    let ceiling = Crossfade::try_from(Duration::from_secs(10)).unwrap();
    assert_eq!(model.settings.audio_settings.crossfade, ceiling);
}

struct CrossfadeRow {
    crossfade: Crossfade,
    direction: Direction,
}

#[rstest]
#[case::below_its_floor(CrossfadeRow {
    crossfade: Crossfade::default(),
    direction: Direction::Previous,
})]
#[case::past_its_ceiling(CrossfadeRow {
    crossfade: Crossfade::try_from(Duration::from_secs(10)).unwrap(),
    direction: Direction::Next,
})]
fn stepping_crossfade_past_either_end_is_refused(#[case] row: CrossfadeRow) {
    let CrossfadeRow {
        crossfade,
        direction,
    } = row;
    let mut model = seeded();
    model.settings.audio_settings.crossfade = crossfade;
    let message = Message::Step {
        row: SettingRow::Crossfade,
        direction,
    };

    let result = update(&mut model, message, Moment::default());

    assert_eq!(result, Err(Unhandled));
    assert_eq!(model.settings.audio_settings.crossfade, crossfade);
}

struct ThemeRow {
    direction: Direction,
    walk: &'static [&'static str],
}

#[rstest]
#[case::forward_one(ThemeRow {
    direction: Direction::Next,
    walk: &["solar", "mono", "noir"],
})]
#[case::backward_one(ThemeRow {
    direction: Direction::Previous,
    walk: &["mono", "solar", "noir"],
})]
fn step_row_theme_cycles_model_themes_and_wraps(#[case] row: ThemeRow) {
    let ThemeRow { direction, walk } = row;
    let mut model = seeded();
    for expected in walk {
        let cmd = step(&mut model, SettingRow::Theme, direction);
        let saved = cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::Save(patch)) = effect else {
                return None;
            };
            patch.theme_name.as_ref().map(ThemeName::to_string)
        });
        let selected = cmd.effects().find_map(|effect| {
            let Effect::Config(ConfigCmd::SelectTheme(choice)) = effect else {
                return None;
            };
            Some(choice.to_string())
        });

        assert_eq!(model.themes.theme_choice.to_string(), *expected);
        assert_eq!(saved.as_deref(), Some(*expected));
        assert_eq!(selected.as_deref(), Some(*expected));
        assert!(!cmd.effects().any(|effect| matches!(
            effect,
            Effect::Animate(_) | Effect::WindowColors(_)
        )));
    }
}

#[rstest]
#[case::theme(SettingRow::Theme, |model: &Model| model.themes.theme_choice == ThemeChoice::Auto)]
#[case::output_device(SettingRow::OutputDevice, |model: &Model| model
    .settings
    .audio_settings
    .device
    == OutputDevice::SystemDefault)]
fn step_row_does_nothing_until_the_shell_delivers_a_list(
    #[case] row: SettingRow,
    #[case] is_unchanged: fn(&Model) -> bool,
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

    assert!(is_unchanged(&model));
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
    assert_eq!(
        model.settings.audio_settings.device,
        OutputDevice::SystemDefault
    );

    let cmd = step(&mut model, SettingRow::OutputDevice, Direction::Next);
    assert_eq!(
        model.settings.audio_settings.device,
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
        model.settings.audio_settings.device,
        OutputDevice::Named(device("Headphones"))
    );

    press(&mut model, SettingRow::OutputDevice, Direction::Next);
    assert_eq!(
        model.settings.audio_settings.device,
        OutputDevice::SystemDefault
    );

    press(&mut model, SettingRow::OutputDevice, Direction::Previous);
    assert_eq!(
        model.settings.audio_settings.device,
        OutputDevice::Named(device("Headphones"))
    );
}

#[test]
fn step_row_sleep_presets_cycles_wraps_persists_and_snaps_a_custom_value() {
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
        Some(model.settings.audio_settings.sleep_presets.as_slice()),
        SleepPresets::BUNDLES.first().copied()
    );

    let cmd = step(&mut model, SettingRow::SleepPresets, Direction::Next);
    assert_eq!(
        Some(model.settings.audio_settings.sleep_presets.as_slice()),
        SleepPresets::BUNDLES.get(1).copied()
    );
    assert_eq!(sleep_presets_patch(&cmd), Some(SleepPresets::bundle(1)));

    press(&mut model, SettingRow::SleepPresets, Direction::Previous);
    let wrapped = step(&mut model, SettingRow::SleepPresets, Direction::Previous);
    assert!(
        model
            .settings
            .audio_settings
            .sleep_presets
            .as_slice()
            .is_empty()
    );
    assert_eq!(sleep_presets_patch(&wrapped), Some(SleepPresets::bundle(4)));

    model.settings.audio_settings.sleep_presets =
        SleepPresets::from_minutes(&[100]).unwrap();
    press(&mut model, SettingRow::SleepPresets, Direction::Next);
    assert_eq!(
        Some(model.settings.audio_settings.sleep_presets.as_slice()),
        SleepPresets::BUNDLES.get(1).copied()
    );
}

#[test]
fn step_row_sleep_presets_leaves_the_clamp_to_the_next_cycle() {
    let mut model = seeded();
    model.transport.sleep_timer = Some(kernel::domain::sleep::SleepTimer {
        preset_index: kernel::domain::index::PresetIndex::new(2),
        delay: Duration::from_secs(60),
        deadline_at: Moment::new(Duration::from_secs(60)),
    });

    press(&mut model, SettingRow::SleepPresets, Direction::Previous);
    let armed = model
        .transport
        .sleep_timer
        .map(|timer| timer.preset_index.get());
    send(&mut model, Message::Playback(PlaybackRequest::CycleSleep));

    assert_eq!(armed, Some(2));
    assert!(model.transport.sleep_timer.is_none());
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
fn the_highlighted_row_changes_across_steps_and_stays_through_an_appearance_reload() {
    let mut model = Model::default();
    let cover_mode_field = AppearanceField::CoverMode;

    send(
        &mut model,
        Message::Overlay(OverlayRequest::Open(OverlayName::Settings)),
    );
    navigate_down(&mut model);
    navigate_down(&mut model);
    assert_eq!(
        selected_row(&model),
        Some(SettingRow::Appearance(cover_mode_field))
    );

    for _ in 0..3 {
        let cmd = update(
            &mut model,
            Message::Overlay(OverlayRequest::Settings(SettingRowRequest::Step(
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
            Some(SettingRow::Appearance(cover_mode_field))
        );
    }

    send(
        &mut model,
        Message::Config(ConfigEvent::AppearanceReloaded(AppearanceSettings {
            key_hints: KeyHints::Hidden,
            ..AppearanceSettings::default()
        })),
    );

    assert_eq!(
        selected_row(&model),
        Some(SettingRow::Appearance(cover_mode_field))
    );
}
