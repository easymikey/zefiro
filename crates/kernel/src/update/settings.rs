use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        Choice,
        CustomSetting,
        Direction,
        Model,
        OutputDevice,
        Replaygain,
        SettingId,
        SettingRow,
        Settings,
        SleepPresets,
        ThemeChoice,
        Themes,
    },
    update::{
        error::UpdateError,
        machine::{Machine, Rejected},
    },
};

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
    let message = match row {
        SettingRow::Theme => return Ok(theme_picked(themes, direction)),
        SettingRow::Custom(id) => {
            return Ok(custom_nudged(custom_settings, id, direction));
        }
        SettingRow::Crossfade => SettingsMessage::Crossfade(direction),
        SettingRow::Replaygain => SettingsMessage::ToggleReplaygain,
        SettingRow::OutputDevice => SettingsMessage::OutputDevice(direction),
        SettingRow::SleepPresets => SettingsMessage::SleepPresets(direction),
    };
    Ok(settings.update(message)?)
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
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().theme(next.clone()).build(),
        )),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(next))),
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
    settings.audio.replaygain = match settings.audio.replaygain {
        Replaygain::On => Replaygain::Off,
        Replaygain::Off => Replaygain::On,
    };
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .replaygain(settings.audio.replaygain)
                .build(),
        )),
        Effect::Audio(AudioCmd::SetReplaygain(settings.audio.replaygain)),
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
