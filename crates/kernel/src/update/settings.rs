use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        appearance_rows::{appearance_patch, appearance_rows},
        device::OutputDevice,
        direction::Direction,
        setting_row::{AppearanceSetting, SettingRow},
        settings::{ReplayGain, Settings},
        sleep_presets::SleepPresets,
        theme::{ThemeChoice, Themes},
    },
    update::{config::ConfigParts, machine::Unhandled},
};

pub(crate) fn step_setting(
    config: ConfigParts<'_>,
    row: SettingRow,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let ConfigParts {
        themes, settings, ..
    } = config;
    match row {
        SettingRow::Theme => theme_picked(themes, direction),
        SettingRow::Appearance(field) => appearance_rows(settings.appearance)
            .into_iter()
            .find(|slot| slot.row.field == field)
            .ok_or(Unhandled)
            .map(|slot| appearance_stepped(settings, slot, direction)),
        SettingRow::Crossfade => Ok(step_crossfade(settings, direction)),
        SettingRow::ReplayGain => Ok(step_replay_gain(settings)),
        SettingRow::OutputDevice => step_output_device(settings, direction),
        SettingRow::SleepPresets => step_sleep_presets(settings, direction),
    }
}

fn appearance_stepped(
    settings: &mut Settings,
    slot: AppearanceSetting,
    direction: Direction,
) -> Cmd {
    let option = slot.choice.stepped(slot.row.control, direction);
    let patch = appearance_patch(slot.row.field, option);
    if let Some(patch) = patch {
        settings.appearance = settings.appearance.patched(patch);
    }
    let setting =
        patch.map(|patch| Cmd::from(Effect::Config(ConfigCmd::SetAppearance(patch))));
    let theme = slot
        .row
        .themes
        .get(option.get())
        .cloned()
        .flatten()
        .map(|name| {
            Cmd::from(Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(
                name,
            ))))
        });
    let cue = slot.row.cue.map(Cmd::from);
    [setting, theme, cue]
        .into_iter()
        .flatten()
        .fold(Cmd::none(), Cmd::then)
}

fn theme_picked(themes: &mut Themes, direction: Direction) -> Result<Cmd, Unhandled> {
    let next = themes.stepped(direction).ok_or(Unhandled)?;
    themes.selected = ThemeChoice::Named(next.clone());
    Ok(Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            theme: Some(next.clone()),
            ..ConfigPatch::default()
        })),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(next))),
    ]))
}

fn step_replay_gain(settings: &mut Settings) -> Cmd {
    settings.audio.replay_gain = match settings.audio.replay_gain {
        ReplayGain::On => ReplayGain::Off,
        ReplayGain::Off => ReplayGain::On,
    };
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            replay_gain: Some(settings.audio.replay_gain),
            ..ConfigPatch::default()
        })),
        Effect::Audio(AudioCmd::SetReplayGain(settings.audio.replay_gain)),
    ])
}

fn step_crossfade(settings: &mut Settings, direction: Direction) -> Cmd {
    settings.audio.crossfade = settings.audio.crossfade.step(direction);
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(ConfigPatch {
            crossfade: Some(settings.audio.crossfade),
            ..ConfigPatch::default()
        })),
        Effect::Audio(AudioCmd::SetCrossfade(settings.audio.crossfade)),
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
    let current = settings.audio.device.named().map_or(0, |name| {
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
    settings.audio.device = next.clone();
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
    let presets = settings.audio.sleep_presets.as_slice();
    let current = SleepPresets::bundle_index(presets)
        .unwrap_or_else(|| SleepPresets::nearest_bundle(presets));
    let next_index = direction.wrapped(current, SleepPresets::BUNDLES.len());
    let next = SleepPresets::bundle(next_index).ok_or(Unhandled)?;
    settings.audio.sleep_presets = next.clone();
    Ok(Effect::Config(ConfigCmd::Save(ConfigPatch {
        sleep_presets: Some(next),
        ..ConfigPatch::default()
    }))
    .into())
}
