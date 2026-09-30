use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, DevicePatch, Effect},
    domain::{
        Choice,
        CustomSetting,
        Direction,
        Model,
        OutputDevice,
        Replaygain,
        SLEEP_PRESET_BUNDLES,
        SettingId,
        SettingRow,
        Settings,
        ThemeChoice,
        ThemeName,
        Themes,
    },
    update::{
        error::UpdateError,
        machine::{Machine, Rejected},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Setting(SettingsMessage),
    Theme(Direction),
    Custom(SettingId),
}

impl SettingRow {
    fn target(self, direction: Direction) -> Target {
        match self {
            SettingRow::Theme => Target::Theme(direction),
            SettingRow::Crossfade => {
                Target::Setting(SettingsMessage::Crossfade(direction))
            }
            SettingRow::Replaygain => {
                Target::Setting(SettingsMessage::ToggleReplaygain)
            }
            SettingRow::OutputDevice => {
                Target::Setting(SettingsMessage::OutputDevice(direction))
            }
            SettingRow::SleepPresets => {
                Target::Setting(SettingsMessage::SleepPresets(direction))
            }
            SettingRow::Custom(id) => Target::Custom(id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessage {
    ToggleReplaygain,
    Crossfade(Direction),
    OutputDevice(Direction),
    SleepPresets(Direction),
}

pub(crate) fn adjust(
    model: &mut Model,
    row: SettingRow,
    direction: Direction,
) -> Result<Cmd, UpdateError> {
    let Model {
        themes,
        settings,
        custom_settings,
        ..
    } = model;
    match row.target(direction) {
        Target::Custom(id) => Ok(custom_nudged(custom_settings, id, direction)),
        Target::Setting(message) => Ok(settings.update(message)?),
        Target::Theme(direction) => Ok(theme_picked(themes, direction)),
    }
}

fn custom_nudged(
    custom_settings: &mut [CustomSetting],
    id: SettingId,
    direction: Direction,
) -> Cmd {
    let Some(slot) = custom_settings.iter_mut().find(|slot| slot.custom.id == id)
    else {
        return Cmd::None;
    };
    let option = slot.choice.nudged(slot.custom.control, direction);
    slot.choice = Choice::Option(option);
    let setting = Cmd::from(Effect::Config(ConfigCmd::Setting { id, option }));
    let theme = slot
        .custom
        .themes
        .get(option.get())
        .cloned()
        .flatten()
        .map(|name| {
            Cmd::from(Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(
                name,
            ))))
        });
    let cue = slot.custom.cue.map(Cmd::from);
    [Some(setting), theme, cue]
        .into_iter()
        .flatten()
        .fold(Cmd::None, Cmd::then)
}

fn theme_picked(themes: &mut Themes, direction: Direction) -> Cmd {
    let Some(next) = themes.nudged(direction) else {
        return Cmd::None;
    };
    themes.selected = ThemeChoice::Named(next.clone());
    theme_effects(next)
}

fn theme_effects(name: ThemeName) -> Cmd {
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().theme(name.clone()).build(),
        )),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(name))),
    ])
}

impl Machine for Settings {
    type Message = SettingsMessage;
    type Error = std::convert::Infallible;
    type Effect = Cmd;

    fn transition(
        mut self,
        message: SettingsMessage,
    ) -> Result<(Self, Cmd), Rejected<Self>> {
        let cmd = match message {
            SettingsMessage::ToggleReplaygain => adjust_replaygain(&mut self),
            SettingsMessage::Crossfade(direction) => {
                adjust_crossfade(&mut self, direction)
            }
            SettingsMessage::OutputDevice(direction) => {
                adjust_output_device(&mut self, direction)
            }
            SettingsMessage::SleepPresets(direction) => {
                adjust_sleep_presets(&mut self, direction)
            }
        };
        Ok((self, cmd))
    }
}

fn adjust_replaygain(settings: &mut Settings) -> Cmd {
    settings.replaygain = match settings.replaygain {
        Replaygain::On => Replaygain::Off,
        Replaygain::Off => Replaygain::On,
    };
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .replaygain(settings.replaygain)
                .build(),
        )),
        Effect::Audio(AudioCmd::SetReplaygain(settings.replaygain)),
    ])
}

fn adjust_crossfade(settings: &mut Settings, direction: Direction) -> Cmd {
    settings.crossfade = settings.crossfade.step(direction);
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().crossfade(settings.crossfade).build(),
        )),
        Effect::Audio(AudioCmd::SetCrossfade(settings.crossfade)),
    ])
}

fn adjust_output_device(settings: &mut Settings, direction: Direction) -> Cmd {
    if settings.output_devices.is_empty() {
        return Cmd::None;
    }
    let ring_len = settings.output_devices.len() + 1;
    let current = settings.output_device.named().map_or(0, |name| {
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
    settings.output_device = next.clone();
    let device_patch = next.named().map_or(DevicePatch::SystemDefault, |name| {
        DevicePatch::Named(name.clone())
    });
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().device(device_patch).build(),
        )),
        Effect::Audio(AudioCmd::SetDevice(next)),
    ])
}

fn adjust_sleep_presets(settings: &mut Settings, direction: Direction) -> Cmd {
    let bundles = &SLEEP_PRESET_BUNDLES;
    let current = bundles
        .index_of(&settings.sleep_presets)
        .unwrap_or_else(|| bundles.nearest_index(&settings.sleep_presets));
    let next_index = direction.wrapped(current, bundles.bundles.len());
    let Some(next) = bundles.bundles.get(next_index).copied() else {
        return Cmd::None;
    };
    let next = next.to_vec();
    settings.sleep_presets = next.clone().into_boxed_slice();
    Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder().sleep_presets(next).build(),
    ))
    .into()
}
