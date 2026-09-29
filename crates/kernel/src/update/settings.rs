use crate::{
    cmd::{AudioCmd, Cmd, ConfigCmd, ConfigPatch, DevicePatch, Effect},
    domain::{
        Choice,
        CustomSetting,
        Model,
        Nudge,
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
        machine::{Machine, Never, Rejected},
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Setting(SettingsMessage),
    Theme(Nudge),
    Custom(SettingId),
}

impl SettingRow {
    fn target(self, nudge: Nudge) -> Target {
        match self {
            SettingRow::Theme => Target::Theme(nudge),
            SettingRow::Crossfade => Target::Setting(SettingsMessage::Crossfade(nudge)),
            SettingRow::Replaygain => {
                Target::Setting(SettingsMessage::ToggleReplaygain)
            }
            SettingRow::OutputDevice => {
                Target::Setting(SettingsMessage::OutputDevice(nudge))
            }
            SettingRow::SleepPresets => {
                Target::Setting(SettingsMessage::SleepPresets(nudge))
            }
            SettingRow::Custom(id) => Target::Custom(id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessage {
    ToggleReplaygain,
    Crossfade(Nudge),
    OutputDevice(Nudge),
    SleepPresets(Nudge),
}

pub(crate) fn adjust(
    model: &mut Model,
    row: SettingRow,
    nudge: Nudge,
) -> Result<Cmd, Rejection> {
    let Model {
        themes,
        settings,
        custom_rows,
        ..
    } = model;
    match row.target(nudge) {
        Target::Custom(id) => Ok(custom_nudged(custom_rows, id, nudge)),
        Target::Setting(message) => Ok(settings.update(message)?),
        Target::Theme(nudge) => Ok(theme_picked(themes, nudge)),
    }
}

fn custom_nudged(
    custom_rows: &mut [CustomSetting],
    id: SettingId,
    nudge: Nudge,
) -> Cmd {
    let Some(slot) = custom_rows.iter_mut().find(|slot| slot.spec.id == id) else {
        return Cmd::None;
    };
    let option = slot.choice.nudged(slot.spec.control, nudge);
    slot.choice = Choice::Option(option);
    let setting = Cmd::from(Effect::Setting { id, option });
    let theme = slot
        .spec
        .themes
        .get(option.get())
        .cloned()
        .flatten()
        .map(|name| {
            Cmd::from(Effect::Config(ConfigCmd::SelectTheme(ThemeChoice::Named(
                name,
            ))))
        });
    let cue = slot.spec.cue.map(Cmd::from);
    [Some(setting), theme, cue]
        .into_iter()
        .flatten()
        .fold(Cmd::None, Cmd::then)
}

fn theme_picked(themes: &mut Themes, nudge: Nudge) -> Cmd {
    let Some(next) = themes.nudged(nudge) else {
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
    type Rejection = Never;
    type Effect = Cmd;

    fn transition(
        mut self,
        message: SettingsMessage,
    ) -> Result<(Self, Cmd), Rejected<Self>> {
        let cmd = match message {
            SettingsMessage::ToggleReplaygain => adjust_replaygain(&mut self),
            SettingsMessage::Crossfade(nudge) => {
                adjust_crossfade(&mut self, delta(nudge))
            }
            SettingsMessage::OutputDevice(nudge) => {
                adjust_output_device(&mut self, delta(nudge))
            }
            SettingsMessage::SleepPresets(nudge) => {
                adjust_sleep_presets(&mut self, delta(nudge))
            }
        };
        Ok((self, cmd))
    }
}

fn delta(nudge: Nudge) -> i64 {
    match nudge {
        Nudge::Up => 1,
        Nudge::Down => -1,
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

fn adjust_crossfade(settings: &mut Settings, delta: i64) -> Cmd {
    settings.crossfade = settings.crossfade.step(delta);
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().crossfade(settings.crossfade).build(),
        )),
        Effect::Audio(AudioCmd::SetCrossfade(settings.crossfade)),
    ])
}

fn adjust_output_device(settings: &mut Settings, delta: i64) -> Cmd {
    if settings.output_devices.is_empty() {
        return Cmd::None;
    }
    let ring_len = settings.output_devices.len() + 1;
    let current = settings.output_device.as_ref().map_or(0, |name| {
        settings
            .output_devices
            .iter()
            .position(|device| &device.name == name)
            .map_or(0, |index| index + 1)
    });
    let next_index = wrapped_index(current, delta, ring_len);
    let next = next_index
        .checked_sub(1)
        .and_then(|previous_index| settings.output_devices.get(previous_index))
        .map(|device| device.name.clone());
    settings.output_device = next.clone();
    let device_patch = next.as_ref().map_or(DevicePatch::SystemDefault, |name| {
        DevicePatch::Named(name.clone())
    });
    Cmd::Batch(vec![
        Effect::Config(ConfigCmd::Save(
            ConfigPatch::builder().device(device_patch).build(),
        )),
        Effect::Audio(AudioCmd::SetDevice(next)),
    ])
}

fn adjust_sleep_presets(settings: &mut Settings, delta: i64) -> Cmd {
    let bundles = &SLEEP_PRESET_BUNDLES;
    let current = bundles
        .index_of(&settings.sleep_presets)
        .unwrap_or_else(|| bundles.nearest_index(&settings.sleep_presets));
    let next_index = wrapped_index(current, delta, bundles.bundles.len());
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

fn wrapped_index(current: usize, delta: i64, len: usize) -> usize {
    if len == 0 {
        return current;
    }
    let (Ok(delta), Ok(len), Ok(current)) = (
        isize::try_from(delta),
        isize::try_from(len),
        isize::try_from(current),
    ) else {
        return current;
    };
    let wrapped = (current + delta).rem_euclid(len);
    usize::try_from(wrapped).unwrap_or(0)
}
