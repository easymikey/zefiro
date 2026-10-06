use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        appearance_rows::{appearance_patch, appearance_row_choices},
        device::OutputDevice,
        direction::Direction,
        setting_row::{AppearanceRowChoice, SettingRow},
        settings::{ReplayGain, Settings},
        sleep_presets::SleepPresets,
        theme::{ThemeChoice, Themes},
    },
    update::{config::ConfigParts, machine::Unhandled},
};

pub(crate) fn step_setting(
    config_parts: ConfigParts<'_>,
    row: SettingRow,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let ConfigParts {
        themes, settings, ..
    } = config_parts;
    match row {
        SettingRow::Theme => step_theme(themes, direction),
        SettingRow::Appearance(field) => {
            appearance_row_choices(settings.appearance_settings)
                .into_iter()
                .find(|appearance_row_choice| appearance_row_choice.row.field == field)
                .ok_or(Unhandled)
                .map(|appearance_row_choice| {
                    step_appearance(settings, appearance_row_choice, direction)
                })
        }
        SettingRow::Crossfade => Ok(step_crossfade(settings, direction)),
        SettingRow::ReplayGain => Ok(step_replay_gain(settings)),
        SettingRow::OutputDevice => step_output_device(settings, direction),
        SettingRow::SleepPresets => step_sleep_presets(settings, direction),
    }
}

fn step_appearance(
    settings: &mut Settings,
    appearance_row_choice: AppearanceRowChoice,
    direction: Direction,
) -> Cmd {
    let option = appearance_row_choice
        .choice
        .stepped(appearance_row_choice.row.control, direction);
    let patch = appearance_patch(appearance_row_choice.row.field, option);
    if let Some(patch) = patch {
        settings.appearance_settings = settings.appearance_settings.patched(patch);
    }
    let setting =
        patch.map(|patch| Cmd::from(Effect::Config(ConfigCmd::SetAppearance(patch))));
    let theme = appearance_row_choice
        .row
        .theme_names
        .get(option.get())
        .cloned()
        .flatten()
        .map(|name| {
            Cmd::from(Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(
                name,
            ))))
        });
    let cue = appearance_row_choice.row.cue.map(Cmd::from);
    [setting, theme, cue]
        .into_iter()
        .flatten()
        .fold(Cmd::none(), Cmd::then)
}

fn step_theme(themes: &mut Themes, direction: Direction) -> Result<Cmd, Unhandled> {
    let next = themes.stepped(direction).ok_or(Unhandled)?;
    themes.theme_choice = ThemeChoice::Named(next.clone());
    Ok(Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            theme_name: Some(next.clone()),
            ..ConfigPatch::default()
        })),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(next))),
    ]))
}

fn step_replay_gain(settings: &mut Settings) -> Cmd {
    settings.audio_settings.replay_gain = match settings.audio_settings.replay_gain {
        ReplayGain::On => ReplayGain::Off,
        ReplayGain::Off => ReplayGain::On,
    };
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            replay_gain: Some(settings.audio_settings.replay_gain),
            ..ConfigPatch::default()
        })),
        Effect::Audio(AudioCmd::SetReplayGain(settings.audio_settings.replay_gain)),
    ])
}

fn step_crossfade(settings: &mut Settings, direction: Direction) -> Cmd {
    settings.audio_settings.crossfade =
        settings.audio_settings.crossfade.step(direction);
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            crossfade: Some(settings.audio_settings.crossfade),
            ..ConfigPatch::default()
        })),
        Effect::Audio(AudioCmd::SetCrossfade(settings.audio_settings.crossfade)),
    ])
}

fn step_output_device(
    settings: &mut Settings,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    if settings.output_devices.is_empty() {
        return Err(Unhandled);
    }
    let ring_len = settings.output_devices.len() + 1;
    let current = settings.audio_settings.device.named().map_or(0, |name| {
        settings
            .output_devices
            .iter()
            .position(|device| &device.name == name)
            .map_or(0, |index| index + 1)
    });
    let next_index = direction.wrapped(current, ring_len);
    let next = next_index
        .checked_sub(1)
        .and_then(|previous_index| settings.output_devices.get(previous_index))
        .map_or(OutputDevice::SystemDefault, |device| {
            OutputDevice::Named(device.name.clone())
        });
    settings.audio_settings.device = next.clone();
    Ok(Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            device: Some(next.clone()),
            ..ConfigPatch::default()
        })),
        Effect::Audio(AudioCmd::SetDevice(next)),
    ]))
}

fn step_sleep_presets(
    settings: &mut Settings,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let presets = settings.audio_settings.sleep_presets.as_slice();
    let current = SleepPresets::bundle_index(presets)
        .unwrap_or_else(|| SleepPresets::nearest_bundle(presets));
    let next_index = direction.wrapped(current, SleepPresets::BUNDLES.len());
    let next = SleepPresets::bundle(next_index).ok_or(Unhandled)?;
    settings.audio_settings.sleep_presets = next.clone();
    Ok(Effect::Config(ConfigCmd::Save(ConfigPatch {
        sleep_presets: Some(next),
        ..ConfigPatch::default()
    }))
    .into())
}
