use std::{path::Path, time::Duration};

use kernel::domain::{
    appearance::{
        Animations,
        AppearancePreset,
        AppearanceSettings,
        CoverBrackets,
        FormatChips,
        KeyHints,
        ProgressTime,
        preset_of,
    },
    appearance_rows::{COVER_MODES, LAYOUT_MODES, SPEED_CHIPS},
    bounded::Bounded,
    crossfade::Crossfade,
    device::ListedDevice,
    setting_row::{AppearanceField, AppearanceSetting, SettingRow},
    settings::ReplayGain,
    sleep_presets::SleepPresets,
    theme::ThemeName,
    time::SECONDS_PER_MINUTE,
};
use unicode_width::UnicodeWidthStr;

use crate::primitive::glyphs;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingsView<'a> {
    pub(crate) crossfade: Crossfade,
    pub(crate) replay_gain: ReplayGain,
    pub(crate) theme: &'a str,
    pub(crate) themes: &'a [ThemeName],
    pub(crate) sleep_presets: &'a [Duration],
    pub(crate) music_dir: &'a Path,
    pub(crate) home: Option<&'a Path>,
    pub(crate) output_device: Option<&'a str>,
    pub(crate) output_devices: &'a [ListedDevice],
    pub(crate) appearance: AppearanceSettings,
    pub(crate) appearance_rows: &'a [AppearanceSetting],
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
        SettingRow::Appearance(field) => appearance_label(field),
    }
}

fn appearance_label(field: AppearanceField) -> &'static str {
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

pub(crate) fn value_text(row: SettingRow, view: &SettingsView<'_>) -> String {
    match row {
        SettingRow::Theme => format_pick(view.theme),
        SettingRow::Crossfade => format_duration_step(view.crossfade.get()),
        SettingRow::ReplayGain => format_toggle(Toggle::from(view.replay_gain)),
        SettingRow::OutputDevice => {
            let name = view
                .output_device
                .unwrap_or(glyphs::settings::OUTPUT_DEVICE_DEFAULT);
            format_pick(name)
        }
        SettingRow::SleepPresets => {
            format_pick(&format_sleep_presets_label(view.sleep_presets))
        }
        SettingRow::Appearance(field) => appearance_value_text(field, view.appearance),
    }
}

fn appearance_value_text(
    field: AppearanceField,
    appearance: AppearanceSettings,
) -> String {
    match field {
        AppearanceField::Preset => format_pick(preset_label(preset_of(appearance))),
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

fn preset_label(preset: Option<AppearancePreset>) -> &'static str {
    match preset {
        Some(AppearancePreset::Stock) => "default",
        Some(AppearancePreset::Noir) => "noir",
        None => "custom",
    }
}

pub(crate) fn max_value_width(row: SettingRow, view: &SettingsView<'_>) -> usize {
    match row {
        SettingRow::Theme => widest_pick(view.themes.iter().map(ThemeName::to_string)),
        SettingRow::Crossfade => format_duration_step(Crossfade::MAX).width(),
        SettingRow::ReplayGain => widest_toggle(),
        SettingRow::OutputDevice => widest_pick(
            view.output_devices
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
        SettingRow::Appearance(field) => appearance_value_width(field),
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

fn appearance_value_width(field: AppearanceField) -> usize {
    match field {
        AppearanceField::Preset => widest_pick(
            [
                Some(AppearancePreset::Stock),
                Some(AppearancePreset::Noir),
                None,
            ]
            .into_iter()
            .map(|preset| preset_label(preset).to_string()),
        ),
        AppearanceField::CoverMode => {
            widest_pick(COVER_MODES.iter().map(ToString::to_string))
        }
        AppearanceField::SpeedChip => {
            widest_pick(SPEED_CHIPS.iter().map(ToString::to_string))
        }
        AppearanceField::LayoutMode => {
            widest_pick(LAYOUT_MODES.iter().map(ToString::to_string))
        }
        AppearanceField::CoverBrackets
        | AppearanceField::FormatChips
        | AppearanceField::ProgressRemaining
        | AppearanceField::KeyHints
        | AppearanceField::Animations => widest_toggle(),
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

fn format_sleep_presets_label(presets: &[Duration]) -> String {
    if presets.is_empty() {
        return glyphs::settings::SLEEP_OFF.to_string();
    }
    presets
        .iter()
        .map(|preset| {
            format!(
                "{}{}",
                preset.as_secs() / SECONDS_PER_MINUTE,
                glyphs::settings::MINUTE_UNIT
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
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
    pub(crate) fn music_dir_label(&self) -> String {
        self.home.map_or_else(
            || self.music_dir.display().to_string(),
            |home| abbreviate_home(self.music_dir, home),
        )
    }
}

#[must_use]
fn abbreviate_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use kernel::domain::{
        appearance::CoverMode,
        appearance_rows::APPEARANCE_ROWS,
        setting_row::{AppearanceField, SettingRow},
        settings::ReplayGain,
    };
    use rstest::rstest;

    use crate::overlay::settings::{
        test_support::{appearance_rows, settings_values},
        view::{
            SettingsView,
            abbreviate_home,
            format_sleep_presets_label,
            settings_label,
            value_text,
        },
    };

    #[rstest]
    #[case::under_home(
        "/Users/test/Desktop/apple-music",
        "/Users/test",
        "~/Desktop/apple-music"
    )]
    #[case::equal_to_home("/Users/test", "/Users/test", "~")]
    #[case::outside_home("/mnt/music", "/Users/test", "/mnt/music")]
    fn a_path_under_home_starts_with_a_tilde(
        #[case] path: &str,
        #[case] home: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(abbreviate_home(Path::new(path), Path::new(home)), expected);
    }

    #[test]
    fn sleep_presets_label_renders_minutes_or_off() {
        assert_eq!(
            format_sleep_presets_label(&[
                Duration::from_secs(15 * 60),
                Duration::from_secs(30 * 60),
                Duration::from_secs(60 * 60),
            ]),
            "15m, 30m, 60m"
        );
        assert_eq!(format_sleep_presets_label(&[]), "off");
    }

    #[test]
    fn toggle_on_off_render_distinct_glyphs() {
        let custom = appearance_rows();
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
        let custom = appearance_rows();
        let view = settings_values(&custom);
        assert!(value_text(SettingRow::Theme, &view).contains("noir"));
    }

    #[test]
    fn a_custom_row_renders_its_appearance_fields_label_and_value() {
        let custom = appearance_rows();
        let view = settings_values(&custom);
        let cover_mode_row = SettingRow::Appearance(
            APPEARANCE_ROWS
                .into_iter()
                .find(|row| row.field == AppearanceField::CoverMode)
                .unwrap()
                .field,
        );
        assert_eq!(settings_label(cover_mode_row), "Cover mode");
        assert!(
            value_text(cover_mode_row, &view)
                .contains(&CoverMode::default().to_string())
        );
    }
}
