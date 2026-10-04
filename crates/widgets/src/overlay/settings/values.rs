use std::{path::Path, time::Duration};

use kernel::{
    Bounded,
    domain::{
        AppearanceSetting,
        Crossfade,
        ListedDevice,
        ReplayGain,
        SettingRow,
        SleepPresets,
        ThemeName,
        appearance::{
            Animations,
            AppearancePatch,
            AppearancePreset,
            AppearanceSettings,
            CoverBrackets,
            FormatChips,
            KeyHints,
            ProgressTime,
            preset_of,
        },
        appearance_rows::{AppearanceField, appearance_patch, appearance_row},
        format_sleep_presets_label,
    },
};
use unicode_width::UnicodeWidthStr;

use crate::primitive::glyphs;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingsView<'a> {
    pub crossfade: Crossfade,
    pub replay_gain: ReplayGain,
    pub theme: &'a str,
    pub themes: &'a [ThemeName],
    pub sleep_presets: &'a [Duration],
    pub music_dir: &'a Path,
    pub home: Option<&'a Path>,
    pub output_device: Option<&'a str>,
    pub output_devices: &'a [ListedDevice],
    pub appearance: AppearanceSettings,
    pub appearance_settings: &'a [AppearanceSetting],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    On,
    Off,
}

impl From<ReplayGain> for Toggle {
    fn from(replay_gain: ReplayGain) -> Self {
        match replay_gain {
            ReplayGain::On => Toggle::On,
            ReplayGain::Off => Toggle::Off,
        }
    }
}

impl From<CoverBrackets> for Toggle {
    fn from(brackets: CoverBrackets) -> Self {
        match brackets {
            CoverBrackets::Shown => Toggle::On,
            CoverBrackets::Hidden => Toggle::Off,
        }
    }
}

impl From<FormatChips> for Toggle {
    fn from(chips: FormatChips) -> Self {
        match chips {
            FormatChips::Shown => Toggle::On,
            FormatChips::Hidden => Toggle::Off,
        }
    }
}

impl From<ProgressTime> for Toggle {
    fn from(time: ProgressTime) -> Self {
        match time {
            ProgressTime::Remaining => Toggle::On,
            ProgressTime::Elapsed => Toggle::Off,
        }
    }
}

impl From<KeyHints> for Toggle {
    fn from(hints: KeyHints) -> Self {
        match hints {
            KeyHints::Shown => Toggle::On,
            KeyHints::Hidden => Toggle::Off,
        }
    }
}

impl From<Animations> for Toggle {
    fn from(animations: Animations) -> Self {
        match animations {
            Animations::On => Toggle::On,
            Animations::Off => Toggle::Off,
        }
    }
}

pub(crate) fn settings_label(row: SettingRow) -> &'static str {
    match row {
        SettingRow::Theme => "Theme",
        SettingRow::Crossfade => "Crossfade",
        SettingRow::ReplayGain => "ReplayGain",
        SettingRow::OutputDevice => "Output device",
        SettingRow::SleepPresets => "Sleep presets",
        SettingRow::Appearance(id) => custom_label(id),
    }
}

fn custom_label(id: AppearanceField) -> &'static str {
    appearance_row(id).map_or("", |row| appearance_field_label(row.field))
}

fn appearance_field_label(field: AppearanceField) -> &'static str {
    match field {
        AppearanceField::Preset => "Preset",
        AppearanceField::CoverMode => "Cover mode",
        AppearanceField::CoverBrackets => "Cover brackets",
        AppearanceField::FormatChips => "Format chips",
        AppearanceField::SpeedChip => "Speed chip",
        AppearanceField::ProgressRemaining => "Progress remaining",
        AppearanceField::KeyHints => "Key hints",
        AppearanceField::Animations => "Animations",
        AppearanceField::LayoutMode => "Layout",
    }
}

pub(crate) fn value_text(row: SettingRow, values: &SettingsView<'_>) -> String {
    match row {
        SettingRow::Theme => format_pick(values.theme),
        SettingRow::Crossfade => format_duration_step(values.crossfade.get()),
        SettingRow::ReplayGain => format_toggle(Toggle::from(values.replay_gain)),
        SettingRow::OutputDevice => {
            let name = values
                .output_device
                .unwrap_or(glyphs::settings::OUTPUT_DEVICE_DEFAULT);
            format_pick(name)
        }
        SettingRow::SleepPresets => {
            format_pick(&format_sleep_presets_label(values.sleep_presets))
        }
        SettingRow::Appearance(id) => custom_value_text(id, values),
    }
}

fn custom_value_text(id: AppearanceField, values: &SettingsView<'_>) -> String {
    let Some(row) = appearance_row(id) else {
        return String::new();
    };
    let appearance = values.appearance;
    match row.field {
        AppearanceField::Preset => format_pick(preset_label(appearance)),
        AppearanceField::CoverMode => format_pick(&appearance.cover_mode.to_string()),
        AppearanceField::CoverBrackets => {
            format_toggle(Toggle::from(appearance.cover_brackets))
        }
        AppearanceField::FormatChips => {
            format_toggle(Toggle::from(appearance.format_chips))
        }
        AppearanceField::SpeedChip => format_pick(&appearance.speed_chip.to_string()),
        AppearanceField::ProgressRemaining => {
            format_toggle(Toggle::from(appearance.progress_time))
        }
        AppearanceField::KeyHints => format_toggle(Toggle::from(appearance.key_hints)),
        AppearanceField::Animations => {
            format_toggle(Toggle::from(appearance.animations))
        }
        AppearanceField::LayoutMode => format_pick(&appearance.layout_mode.to_string()),
    }
}

fn preset_label(appearance: AppearanceSettings) -> &'static str {
    match preset_of(appearance) {
        Some(AppearancePreset::Stock) => "default",
        Some(AppearancePreset::Noir) => "noir",
        None => "custom",
    }
}

pub(crate) fn max_value_width(row: SettingRow, values: &SettingsView<'_>) -> usize {
    match row {
        SettingRow::Theme => {
            widest_pick(values.themes.iter().map(ThemeName::to_string))
        }
        SettingRow::Crossfade => format_duration_step(Crossfade::MAX).width(),
        SettingRow::ReplayGain => widest_toggle(),
        SettingRow::OutputDevice => widest_pick(
            values
                .output_devices
                .iter()
                .map(|device| device.name.to_string())
                .chain(std::iter::once(
                    glyphs::settings::OUTPUT_DEVICE_DEFAULT.to_string(),
                )),
        ),
        SettingRow::SleepPresets => widest_pick(
            SleepPresets::BUNDLES
                .iter()
                .map(|bundle| format_sleep_presets_label(bundle)),
        ),
        SettingRow::Appearance(id) => custom_max_value_width(id, values),
    }
}

fn widest_toggle() -> usize {
    format_toggle(Toggle::On)
        .width()
        .max(format_toggle(Toggle::Off).width())
}

fn widest_pick(labels: impl Iterator<Item = String>) -> usize {
    labels
        .map(|label| format_pick(&label).width())
        .max()
        .unwrap_or(0)
}

fn custom_max_value_width(id: AppearanceField, values: &SettingsView<'_>) -> usize {
    let Some(row) = appearance_row(id) else {
        return 0;
    };
    let count = row.control.count();
    (0..count.get())
        .filter_map(|position| count.index(position))
        .filter_map(|option| appearance_patch(id, option))
        .map(|patch| {
            custom_value_text(id, &with_patched_appearance(values, patch)).width()
        })
        .max()
        .unwrap_or(0)
}

fn with_patched_appearance<'a>(
    values: &SettingsView<'a>,
    patch: AppearancePatch,
) -> SettingsView<'a> {
    SettingsView {
        appearance: values.appearance.patched(patch),
        ..*values
    }
}

fn format_toggle(toggle: Toggle) -> String {
    match toggle {
        Toggle::On => glyphs::settings::TOGGLE_ON.to_string(),
        Toggle::Off => glyphs::settings::TOGGLE_OFF.to_string(),
    }
}

fn format_duration_step(duration: Duration) -> String {
    format!(
        "{:.1}{}",
        duration.as_secs_f64(),
        glyphs::settings::DURATION_UNIT
    )
}

fn format_pick(current: &str) -> String {
    format!(
        "{}{current}{}",
        glyphs::settings::PICK_LEFT,
        glyphs::settings::PICK_RIGHT
    )
}

impl<'a> SettingsView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &crate::scene::Scene<'a>) -> Self {
        let audio = &scene.settings.audio;
        Self {
            crossfade: audio.crossfade,
            replay_gain: audio.replay_gain,
            theme: crate::scene::theme_label(&scene.themes.selected),
            themes: &scene.themes.names,
            sleep_presets: audio.sleep_presets.as_slice(),
            music_dir: scene.music_dir,
            home: scene.home,
            output_device: audio.device.named().map(kernel::domain::DeviceName::as_str),
            output_devices: &scene.settings.output_devices,
            appearance: scene.appearance().settings,
            appearance_settings: scene.appearance_settings,
        }
    }

    #[must_use]
    pub(crate) fn music_dir_label(&self) -> String {
        self.home.map_or_else(
            || self.music_dir.display().to_string(),
            |home| crate::scene::abbreviate_home(self.music_dir, home),
        )
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        ReplayGain,
        SettingRow,
        appearance::CoverMode,
        appearance_rows::{APPEARANCE_ROWS, AppearanceField},
    };

    use crate::overlay::settings::{
        test_support::{appearance_settings, settings_values},
        values::{SettingsView, settings_label, value_text},
    };

    #[test]
    fn toggle_on_off_render_distinct_glyphs() {
        let custom = appearance_settings();
        let on = settings_values(&custom);
        let off = SettingsView {
            replay_gain: ReplayGain::Off,
            ..on
        };
        assert_ne!(
            value_text(SettingRow::ReplayGain, &on),
            value_text(SettingRow::ReplayGain, &off)
        );
    }

    #[test]
    fn pick_row_shows_current_theme() {
        let custom = appearance_settings();
        let values = settings_values(&custom);
        assert!(value_text(SettingRow::Theme, &values).contains("noir"));
    }

    #[test]
    fn a_custom_row_renders_its_appearance_fields_label_and_value() {
        let custom = appearance_settings();
        let values = settings_values(&custom);
        let cover_mode_row = SettingRow::Appearance(
            APPEARANCE_ROWS
                .into_iter()
                .find(|row| row.field == AppearanceField::CoverMode)
                .unwrap()
                .field,
        );
        assert_eq!(settings_label(cover_mode_row), "Cover mode");
        assert!(
            value_text(cover_mode_row, &values)
                .contains(&CoverMode::default().to_string())
        );
    }
}
