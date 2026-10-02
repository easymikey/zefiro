use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        AppearanceSetting,
        Choice,
        Direction,
        OutputDevice,
        ReplayGain,
        SettingRow,
        Settings,
        SleepPresets,
        ThemeChoice,
        Themes,
        appearance_rows::AppearanceField,
    },
    update::config::ConfigParts,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessage {
    ToggleReplayGain,
    Crossfade(Direction),
    OutputDevice(Direction),
    SleepPresets(Direction),
}

pub(crate) fn adjust(
    config: ConfigParts<'_>,
    row: SettingRow,
    direction: Direction,
) -> Cmd {
    let ConfigParts {
        themes,
        settings,
        appearance_settings,
        ..
    } = config;
    let message = match row {
        SettingRow::Theme => return theme_picked(themes, direction),
        SettingRow::Appearance(field) => {
            return appearance_stepped(appearance_settings, field, direction);
        }
        SettingRow::Crossfade => SettingsMessage::Crossfade(direction),
        SettingRow::ReplayGain => SettingsMessage::ToggleReplayGain,
        SettingRow::OutputDevice => SettingsMessage::OutputDevice(direction),
        SettingRow::SleepPresets => SettingsMessage::SleepPresets(direction),
    };
    settings.apply(message)
}

fn appearance_stepped(
    appearance_settings: &mut [AppearanceSetting],
    field: AppearanceField,
    direction: Direction,
) -> Cmd {
    let Some(slot) = appearance_settings
        .iter_mut()
        .find(|slot| slot.row.field == field)
    else {
        return Cmd::None;
    };
    let option = slot.choice.stepped(slot.row.control, direction);
    slot.choice = Choice::Option(option);
    let setting = Cmd::from(Effect::Config(ConfigCmd::Setting { field, option }));
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
    [Some(setting), theme, cue]
        .into_iter()
        .flatten()
        .fold(Cmd::None, Cmd::then)
}

fn theme_picked(themes: &mut Themes, direction: Direction) -> Cmd {
    let Some(next) = themes.stepped(direction) else {
        return Cmd::None;
    };
    themes.selected = ThemeChoice::Named(next.clone());
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().theme(next.clone()).build(),
        )),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(next))),
    ])
}

impl Settings {
    pub fn apply(&mut self, message: SettingsMessage) -> Cmd {
        match message {
            SettingsMessage::ToggleReplayGain => adjust_replay_gain(self),
            SettingsMessage::Crossfade(direction) => adjust_crossfade(self, direction),
            SettingsMessage::OutputDevice(direction) => {
                adjust_output_device(self, direction)
            }
            SettingsMessage::SleepPresets(direction) => {
                adjust_sleep_presets(self, direction)
            }
        }
    }
}

fn adjust_replay_gain(settings: &mut Settings) -> Cmd {
    settings.audio.replay_gain = match settings.audio.replay_gain {
        ReplayGain::On => ReplayGain::Off,
        ReplayGain::Off => ReplayGain::On,
    };
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .replay_gain(settings.audio.replay_gain)
                .build(),
        )),
        Effect::Audio(AudioCmd::SetReplayGain(settings.audio.replay_gain)),
    ])
}

fn adjust_crossfade(settings: &mut Settings, direction: Direction) -> Cmd {
    settings.audio.crossfade = settings.audio.crossfade.step(direction);
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .crossfade(settings.audio.crossfade)
                .build(),
        )),
        Effect::Audio(AudioCmd::SetCrossfade(settings.audio.crossfade)),
    ])
}

fn adjust_output_device(settings: &mut Settings, direction: Direction) -> Cmd {
    if settings.output_devices.is_empty() {
        return Cmd::None;
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
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().device(next.clone()).build(),
        )),
        Effect::Audio(AudioCmd::SetDevice(next)),
    ])
}

fn adjust_sleep_presets(settings: &mut Settings, direction: Direction) -> Cmd {
    let presets = settings.audio.sleep_presets.as_slice();
    let current = SleepPresets::bundle_index(presets)
        .unwrap_or_else(|| SleepPresets::nearest_bundle(presets));
    let next_index = direction.wrapped(current, SleepPresets::BUNDLES.len());
    let Some(next) = SleepPresets::bundle(next_index) else {
        return Cmd::None;
    };
    settings.audio.sleep_presets = next.clone();
    Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder().sleep_presets(next).build(),
    ))
    .into()
}
