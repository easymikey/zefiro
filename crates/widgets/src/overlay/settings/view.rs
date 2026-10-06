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
    setting_row::{AppearanceField, SettingRow},
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
    pub(crate) theme_names: &'a [ThemeName],
    pub(crate) sleep_presets: &'a [Duration],
    pub(crate) music_dir: &'a Path,
    pub(crate) home_dir: Option<&'a Path>,
    pub(crate) output_device_name: Option<&'a str>,
    pub(crate) output_devices: &'a [ListedDevice],
    pub(crate) appearance_settings: AppearanceSettings,
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
    fn from(progress_time: ProgressTime) -> Self {
        match progress_time {
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
        AppearanceField::ProgressTime => "Progress remaining",
        AppearanceField::KeyHints => "Key hints",
        AppearanceField::Animations => "Animations",
        AppearanceField::LayoutMode => "Layout",
    }
}

pub(crate) fn value_text(row: SettingRow, view: &SettingsView<'_>) -> String {
    match row {
        SettingRow::Theme => pick_text(view.theme),
        SettingRow::Crossfade => duration_step_text(view.crossfade.get()),
        SettingRow::ReplayGain => toggle_text(Toggle::from(view.replay_gain)),
        SettingRow::OutputDevice => {
            let name = view
                .output_device_name
                .unwrap_or(glyphs::settings::OUTPUT_DEVICE_DEFAULT);
            pick_text(name)
        }
        SettingRow::SleepPresets => pick_text(&sleep_presets_text(view.sleep_presets)),
        SettingRow::Appearance(field) => {
            appearance_value_text(field, view.appearance_settings)
        }
    }
}

fn appearance_value_text(
    field: AppearanceField,
    appearance_settings: AppearanceSettings,
) -> String {
    match field {
        AppearanceField::Preset => {
            pick_text(preset_label(preset_of(appearance_settings)))
        }
        AppearanceField::CoverMode => {
            pick_text(&appearance_settings.cover_mode.to_string())
        }
        AppearanceField::CoverBrackets => {
            toggle_text(Toggle::from(appearance_settings.cover_brackets))
        }
        AppearanceField::FormatChips => {
            toggle_text(Toggle::from(appearance_settings.format_chips))
        }
        AppearanceField::SpeedChip => {
            pick_text(&appearance_settings.speed_chip.to_string())
        }
        AppearanceField::ProgressTime => {
            toggle_text(Toggle::from(appearance_settings.progress_time))
        }
        AppearanceField::KeyHints => {
            toggle_text(Toggle::from(appearance_settings.key_hints))
        }
        AppearanceField::Animations => {
            toggle_text(Toggle::from(appearance_settings.animations))
        }
        AppearanceField::LayoutMode => {
            pick_text(&appearance_settings.layout_mode.to_string())
        }
    }
}

fn preset_label(preset: Option<AppearancePreset>) -> &'static str {
    match preset {
        Some(AppearancePreset::Stock) => "default",
        Some(AppearancePreset::Noir) => "noir",
        None => "custom",
    }
}

pub(crate) fn widest_value(row: SettingRow, view: &SettingsView<'_>) -> usize {
    match row {
        SettingRow::Theme => {
            widest_pick(view.theme_names.iter().map(ThemeName::to_string))
        }
        SettingRow::Crossfade => duration_step_text(Crossfade::MAX).width(),
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
                .map(|bundle| sleep_presets_text(bundle)),
        ),
        SettingRow::Appearance(field) => appearance_value_width(field),
    }
}

fn widest_toggle() -> usize {
    toggle_text(Toggle::On)
        .width()
        .max(toggle_text(Toggle::Off).width())
}

fn widest_pick(labels: impl Iterator<Item = String>) -> usize {
    labels
        .map(|label| pick_text(&label).width())
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
        | AppearanceField::ProgressTime
        | AppearanceField::KeyHints
        | AppearanceField::Animations => widest_toggle(),
    }
}

fn toggle_text(toggle: Toggle) -> String {
    match toggle {
        Toggle::On => glyphs::settings::TOGGLE_ON.to_string(),
        Toggle::Off => glyphs::settings::TOGGLE_OFF.to_string(),
    }
}

fn duration_step_text(duration: Duration) -> String {
    format!(
        "{:.1}{}",
        duration.as_secs_f64(),
        glyphs::settings::DURATION_UNIT
    )
}

fn sleep_presets_text(presets: &[Duration]) -> String {
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

fn pick_text(current: &str) -> String {
    format!(
        "{}{current}{}",
        glyphs::settings::PICK_LEFT,
        glyphs::settings::PICK_RIGHT
    )
}

impl<'a> SettingsView<'a> {
    #[must_use]
    pub(crate) fn music_dir_label(&self) -> String {
        self.home_dir.map_or_else(
            || self.music_dir.display().to_string(),
            |home| abbreviate_home(self.music_dir, home),
        )
    }
}

#[must_use]
fn abbreviate_home(path: &Path, home_dir: &Path) -> String {
    match path.strip_prefix(home_dir) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use kernel::domain::{
        appearance::{
            Animations,
            AppearanceSettings,
            CoverBrackets,
            CoverMode,
            FormatChips,
            KeyHints,
            ProgressTime,
        },
        appearance_rows::APPEARANCE_ROWS,
        setting_row::{AppearanceField, SettingRow},
        settings::ReplayGain,
    };
    use rstest::rstest;

    use crate::overlay::settings::{
        test_support::settings_values,
        view::{
            SettingsView,
            abbreviate_home,
            appearance_value_text,
            settings_label,
            sleep_presets_text,
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
    fn sleep_presets_text_renders_minutes_or_off() {
        assert_eq!(
            sleep_presets_text(&[
                Duration::from_secs(15 * 60),
                Duration::from_secs(30 * 60),
                Duration::from_secs(60 * 60),
            ]),
            "15m, 30m, 60m"
        );
        assert_eq!(sleep_presets_text(&[]), "off");
    }

    #[test]
    fn toggle_on_off_render_distinct_glyphs() {
        let on = settings_values();
        let off_view = SettingsView {
            replay_gain: ReplayGain::Off,
            ..on
        };
        assert_ne!(
            value_text(SettingRow::ReplayGain, &on),
            value_text(SettingRow::ReplayGain, &off_view)
        );
    }

    #[test]
    fn settings_toggle_rows_render_their_two_values_apart() {
        let on_appearance_settings = AppearanceSettings {
            cover_brackets: CoverBrackets::Shown,
            format_chips: FormatChips::Shown,
            progress_time: ProgressTime::Remaining,
            key_hints: KeyHints::Shown,
            animations: Animations::On,
            ..AppearanceSettings::default()
        };
        let off_appearance_settings = AppearanceSettings {
            cover_brackets: CoverBrackets::Hidden,
            format_chips: FormatChips::Hidden,
            progress_time: ProgressTime::Elapsed,
            key_hints: KeyHints::Hidden,
            animations: Animations::Off,
            ..on_appearance_settings
        };
        for field in [
            AppearanceField::CoverBrackets,
            AppearanceField::FormatChips,
            AppearanceField::ProgressTime,
            AppearanceField::KeyHints,
            AppearanceField::Animations,
        ] {
            assert_ne!(
                appearance_value_text(field, on_appearance_settings),
                appearance_value_text(field, off_appearance_settings)
            );
        }
    }

    #[test]
    fn pick_row_shows_current_theme() {
        let view = settings_values();
        assert!(value_text(SettingRow::Theme, &view).contains("noir"));
    }

    #[test]
    fn a_custom_row_renders_its_appearance_fields_label_and_value() {
        let view = settings_values();
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
