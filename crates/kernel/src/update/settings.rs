use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        device::OutputDevice,
        direction::Direction,
        setting_row::{AppearanceField, AppearanceSetting, Choice, SettingRow},
        settings::{ReplayGain, Settings},
        sleep_presets::SleepPresets,
        theme::{ThemeChoice, Themes},
    },
    update::{
        config::ConfigParts,
        machine::{Machine, Unhandled},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessage {
    ToggleReplayGain,
    Crossfade(Direction),
    OutputDevice(Direction),
    SleepPresets(Direction),
}

pub(crate) fn step_setting(
    config: ConfigParts<'_>,
    row: SettingRow,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let ConfigParts {
        themes,
        settings,
        appearance_rows,
        ..
    } = config;
    let message = match row {
        SettingRow::Theme => return Ok(theme_picked(themes, direction)),
        SettingRow::Appearance(field) => {
            return Ok(appearance_stepped(appearance_rows, field, direction));
        }
        SettingRow::Crossfade => SettingsMessage::Crossfade(direction),
        SettingRow::ReplayGain => SettingsMessage::ToggleReplayGain,
        SettingRow::OutputDevice => SettingsMessage::OutputDevice(direction),
        SettingRow::SleepPresets => SettingsMessage::SleepPresets(direction),
    };
    settings.transition(message)
}

fn appearance_stepped(
    appearance_rows: &mut [AppearanceSetting],
    field: AppearanceField,
    direction: Direction,
) -> Cmd {
    let Some(slot) = appearance_rows
        .iter_mut()
        .find(|slot| slot.row.field == field)
    else {
        return Cmd::none();
    };
    let option = slot.choice.stepped(slot.row.control, direction);
    slot.choice = Choice::Option(option);
    let setting = Cmd::from(Effect::Config(ConfigCmd::SetAppearance { field, option }));
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
        .fold(Cmd::none(), Cmd::then)
}

fn theme_picked(themes: &mut Themes, direction: Direction) -> Cmd {
    let Some(next) = themes.stepped(direction) else {
        return Cmd::none();
    };
    themes.selected = ThemeChoice::Named(next.clone());
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().theme(next.clone()).build(),
        )),
        Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(next))),
    ])
}

impl Machine for Settings {
    type Message = SettingsMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: SettingsMessage) -> Result<Cmd, Unhandled> {
        Ok(match message {
            SettingsMessage::ToggleReplayGain => step_replay_gain(self),
            SettingsMessage::Crossfade(direction) => step_crossfade(self, direction),
            SettingsMessage::OutputDevice(direction) => {
                step_output_device(self, direction)
            }
            SettingsMessage::SleepPresets(direction) => {
                step_sleep_presets(self, direction)
            }
        })
    }
}

fn step_replay_gain(settings: &mut Settings) -> Cmd {
    settings.audio.replay_gain = match settings.audio.replay_gain {
        ReplayGain::On => ReplayGain::Off,
        ReplayGain::Off => ReplayGain::On,
    };
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .replay_gain(settings.audio.replay_gain)
                .build(),
        )),
        Effect::Audio(AudioCmd::SetReplayGain(settings.audio.replay_gain)),
    ])
}

fn step_crossfade(settings: &mut Settings, direction: Direction) -> Cmd {
    settings.audio.crossfade = settings.audio.crossfade.step(direction);
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder()
                .crossfade(settings.audio.crossfade)
                .build(),
        )),
        Effect::Audio(AudioCmd::SetCrossfade(settings.audio.crossfade)),
    ])
}

fn step_output_device(settings: &mut Settings, direction: Direction) -> Cmd {
    if settings.output_devices.is_empty() {
        return Cmd::none();
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
    Cmd::from_iter([
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().device(next.clone()).build(),
        )),
        Effect::Audio(AudioCmd::SetDevice(next)),
    ])
}

fn step_sleep_presets(settings: &mut Settings, direction: Direction) -> Cmd {
    let presets = settings.audio.sleep_presets.as_slice();
    let current = SleepPresets::bundle_index(presets)
        .unwrap_or_else(|| SleepPresets::nearest_bundle(presets));
    let next_index = direction.wrapped(current, SleepPresets::BUNDLES.len());
    let Some(next) = SleepPresets::bundle(next_index) else {
        return Cmd::none();
    };
    settings.audio.sleep_presets = next.clone();
    Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder().sleep_presets(next).build(),
    ))
    .into()
}
