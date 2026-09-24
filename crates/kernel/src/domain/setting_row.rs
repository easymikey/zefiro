use crate::cmd::Cue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingRow {
    Theme,
    Crossfade,
    Replaygain,
    OutputDevice,
    SleepPresets,
    Custom(SettingId),
}

impl SettingRow {
    #[must_use]
    pub fn all(custom: &[CustomSetting]) -> Vec<SettingRow> {
        let mut rows = Vec::new();
        match custom.split_first() {
            Some((leading, rest)) => {
                rows.push(SettingRow::Custom(leading.id));
                rows.push(SettingRow::Theme);
                rows.extend(rest.iter().map(|slot| SettingRow::Custom(slot.id)));
            }
            None => rows.push(SettingRow::Theme),
        }
        rows.extend(
            SETTINGS
                .iter()
                .map(|spec| spec.row)
                .filter(|row| *row != SettingRow::Theme),
        );
        rows
    }

    #[must_use]
    pub fn control(self, custom: &[CustomSetting]) -> SettingControl {
        match self {
            SettingRow::Custom(id) => custom
                .iter()
                .find(|slot| slot.id == id)
                .map_or(SettingControl::Step, |slot| slot.control),
            SettingRow::Theme
            | SettingRow::Crossfade
            | SettingRow::Replaygain
            | SettingRow::OutputDevice
            | SettingRow::SleepPresets => SETTINGS
                .iter()
                .find(|spec| spec.row == self)
                .map_or(SettingControl::Toggle, |spec| spec.control),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    pub row: SettingRow,
    pub control: SettingControl,
}

pub const SETTINGS: [SettingSpec; 5] = [
    SettingSpec {
        row: SettingRow::Theme,
        control: SettingControl::Cycle(0),
    },
    SettingSpec {
        row: SettingRow::Crossfade,
        control: SettingControl::Step,
    },
    SettingSpec {
        row: SettingRow::Replaygain,
        control: SettingControl::Toggle,
    },
    SettingSpec {
        row: SettingRow::OutputDevice,
        control: SettingControl::Cycle(0),
    },
    SettingSpec {
        row: SettingRow::SleepPresets,
        control: SettingControl::Cycle(0),
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingControl {
    Toggle,
    Cycle(usize),
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomSetting {
    pub id: SettingId,
    pub control: SettingControl,
    pub position: usize,
    pub cue: Option<Cue>,
    pub themes: &'static [Option<&'static str>],
}
