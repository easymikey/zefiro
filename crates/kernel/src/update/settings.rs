use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        appearance::{AppearancePreset, preset_of},
        appearance_rows::{appearance_patch, field_choice},
        device::OutputDevice,
        direction::Direction,
        setting_row::{AppearanceField, SettingRow},
        settings::{ReplayGain, Settings},
        sleep_presets::SleepPresets,
        theme::{ThemeChoice, ThemeName, Themes},
    },
    update::{
        config::ConfigParts,
        machine::{Unhandled, replace},
    },
};

pub(crate) fn step_setting(
    config_parts: ConfigParts<'_>,
    row: SettingRow,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    match row {
        SettingRow::Theme => step_theme(config_parts.themes, direction),
        SettingRow::Appearance(field) => {
            Ok(step_appearance(config_parts, field, direction))
        }
        SettingRow::Crossfade => step_crossfade(config_parts.settings, direction),
        SettingRow::ReplayGain => Ok(step_replay_gain(config_parts.settings)),
        SettingRow::OutputDevice => {
            step_output_device(config_parts.settings, direction)
        }
        SettingRow::SleepPresets => {
            Ok(step_sleep_presets(config_parts.settings, direction))
        }
    }
}

fn step_appearance(
    config_parts: ConfigParts<'_>,
    field: AppearanceField,
    direction: Direction,
) -> Cmd {
    let ConfigParts {
        themes,
        settings,
        workspace: _,
        revisions: _,
        music_dir: _,
    } = config_parts;
    let row = field.row();
    let option = field_choice(field, settings.appearance_settings)
        .stepped(row.control, direction);
    let setting = match appearance_patch(field, option) {
        Some(patch) => {
            settings.appearance_settings = settings.appearance_settings.patched(patch);
            Cmd::from(Effect::Config(ConfigCmd::SetAppearance(patch)))
        }
        None => Cmd::none(),
    };
    let theme = if field == AppearanceField::Preset
        && let Some(name) =
            preset_of(settings.appearance_settings).and_then(AppearancePreset::theme)
    {
        select_theme(themes, name)
    } else {
        Cmd::none()
    };
    let cue = row.cue.map_or_else(Cmd::none, Cmd::from);
    setting.then(theme).then(cue)
}

fn step_theme(themes: &mut Themes, direction: Direction) -> Result<Cmd, Unhandled> {
    let next = themes.stepped(direction).ok_or(Unhandled)?;
    Ok(select_theme(themes, next))
}

fn select_theme(themes: &mut Themes, name: ThemeName) -> Cmd {
    themes.theme_choice = ThemeChoice::Named(name.clone());
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            theme_name: Some(name.clone()),
            ..ConfigPatch::default()
        })),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(name))),
    ])
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

fn step_crossfade(
    settings: &mut Settings,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let crossfade = settings.audio_settings.crossfade.step(direction);
    replace(&mut settings.audio_settings.crossfade, crossfade)?;
    Ok(Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            crossfade: Some(settings.audio_settings.crossfade),
            ..ConfigPatch::default()
        })),
        Effect::Audio(AudioCmd::SetCrossfade(settings.audio_settings.crossfade)),
    ]))
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

fn step_sleep_presets(settings: &mut Settings, direction: Direction) -> Cmd {
    let presets = settings.audio_settings.sleep_presets.as_slice();
    let current = SleepPresets::nearest_bundle(presets);
    let next_index = direction.wrapped(current, SleepPresets::BUNDLES.len());
    let next = SleepPresets::bundle(next_index);
    settings.audio_settings.sleep_presets = next.clone();
    Effect::Config(ConfigCmd::Save(ConfigPatch {
        sleep_presets: Some(next),
        ..ConfigPatch::default()
    }))
    .into()
}
