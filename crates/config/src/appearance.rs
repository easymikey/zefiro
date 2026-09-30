use kernel::domain::ThemeName;
use serde::{Deserialize, Deserializer};
use strum::{EnumIter, EnumString, IntoEnumIterator};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, strum::Display, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum CoverStyle {
    #[default]
    Vinyl,
    Plain,
    Milkdrop,
    Off,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, strum::Display, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum SpeedChip {
    #[default]
    Always,
    Changed,
    Never,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, strum::Display, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LayoutMode {
    #[default]
    Auto,
    Full,
    Compact,
}

pub(crate) fn from_bool<'de, D, Flag>(
    deserializer: D,
    on: Flag,
    off: Flag,
) -> Result<Flag, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(if bool::deserialize(deserializer)? {
        on
    } else {
        off
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverBrackets {
    Shown,
    #[default]
    Hidden,
}

pub(crate) fn cover_brackets<'de, D>(deserializer: D) -> Result<CoverBrackets, D::Error>
where
    D: Deserializer<'de>,
{
    from_bool(deserializer, CoverBrackets::Shown, CoverBrackets::Hidden)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatChips {
    Shown,
    #[default]
    Hidden,
}

pub(crate) fn format_chips<'de, D>(deserializer: D) -> Result<FormatChips, D::Error>
where
    D: Deserializer<'de>,
{
    from_bool(deserializer, FormatChips::Shown, FormatChips::Hidden)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressTime {
    Remaining,
    #[default]
    Elapsed,
}

pub(crate) fn progress_style<'de, D>(deserializer: D) -> Result<ProgressTime, D::Error>
where
    D: Deserializer<'de>,
{
    from_bool(deserializer, ProgressTime::Remaining, ProgressTime::Elapsed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animations {
    #[default]
    On,
    Off,
}

pub(crate) fn animations<'de, D>(deserializer: D) -> Result<Animations, D::Error>
where
    D: Deserializer<'de>,
{
    from_bool(deserializer, Animations::On, Animations::Off)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyHints {
    #[default]
    Shown,
    Hidden,
}

pub(crate) fn key_hints<'de, D>(deserializer: D) -> Result<KeyHints, D::Error>
where
    D: Deserializer<'de>,
{
    from_bool(deserializer, KeyHints::Shown, KeyHints::Hidden)
}

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Appearance {
    pub cover_style: CoverStyle,
    pub cover_brackets: CoverBrackets,
    pub format_chips: FormatChips,
    pub speed_chip: SpeedChip,
    pub progress_time: ProgressTime,
    pub key_hints: KeyHints,
    pub animations: Animations,
    pub layout_mode: LayoutMode,
}

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, EnumIter)]
pub enum AppearancePreset {
    #[default]
    Stock,
    Noir,
}

impl AppearancePreset {
    #[must_use]
    pub const fn theme(self) -> Option<ThemeName> {
        match self {
            AppearancePreset::Stock => None,
            AppearancePreset::Noir => Some(ThemeName::from_static("noir")),
        }
    }
}

pub fn preset_appearance(preset: AppearancePreset) -> Appearance {
    match preset {
        AppearancePreset::Stock => Appearance::default(),
        AppearancePreset::Noir => Appearance {
            cover_style: CoverStyle::Milkdrop,
            cover_brackets: CoverBrackets::Shown,
            format_chips: FormatChips::Shown,
            speed_chip: SpeedChip::Always,
            progress_time: ProgressTime::Remaining,
            key_hints: KeyHints::Shown,
            animations: Animations::On,
            layout_mode: LayoutMode::Auto,
        },
    }
}

#[must_use]
pub fn preset_of(appearance: Appearance) -> Option<AppearancePreset> {
    AppearancePreset::iter().find(|&preset| preset_appearance(preset) == appearance)
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bon::Builder)]
pub struct AppearancePatch {
    #[builder(setters(option_fn(name = with_cover_style)))]
    pub cover_style: Option<CoverStyle>,
    #[builder(setters(option_fn(name = with_cover_brackets)))]
    pub cover_brackets: Option<CoverBrackets>,
    #[builder(setters(option_fn(name = with_format_chips)))]
    pub format_chips: Option<FormatChips>,
    #[builder(setters(option_fn(name = with_speed_chip)))]
    pub speed_chip: Option<SpeedChip>,
    #[builder(setters(option_fn(name = with_progress_time)))]
    pub progress_time: Option<ProgressTime>,
    #[builder(setters(option_fn(name = with_key_hints)))]
    pub key_hints: Option<KeyHints>,
    #[builder(setters(option_fn(name = with_animations)))]
    pub animations: Option<Animations>,
    #[builder(setters(option_fn(name = with_layout_mode)))]
    pub layout_mode: Option<LayoutMode>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::appearance::{
        Appearance,
        AppearancePreset,
        CoverStyle,
        FormatChips,
        LayoutMode,
        SpeedChip,
        preset_appearance,
        preset_of,
    };

    #[test]
    fn the_stock_appearance_is_every_vocabulary_default() {
        insta::assert_debug_snapshot!(Appearance::default());
    }

    #[test]
    fn the_default_preset_is_exactly_the_stock_appearance() {
        assert_eq!(
            preset_appearance(AppearancePreset::Stock),
            Appearance::default()
        );
    }

    #[test]
    fn the_noir_preset_names_every_option_it_changes() {
        insta::assert_debug_snapshot!(preset_appearance(AppearancePreset::Noir));
    }

    #[rstest]
    #[case::stock(Appearance::default(), Some(AppearancePreset::Stock))]
    #[case::noir(
        preset_appearance(AppearancePreset::Noir),
        Some(AppearancePreset::Noir)
    )]
    #[case::a_custom_mix(
        Appearance { format_chips: FormatChips::Shown, ..Appearance::default() },
        None
    )]
    fn preset_of_names_the_preset_an_appearance_came_from(
        #[case] appearance: Appearance,
        #[case] preset: Option<AppearancePreset>,
    ) {
        assert_eq!(preset_of(appearance), preset);
    }

    #[rstest]
    #[case(AppearancePreset::Stock, None)]
    #[case(AppearancePreset::Noir, Some("noir"))]
    fn a_preset_names_the_theme_it_wants(
        #[case] preset: AppearancePreset,
        #[case] theme: Option<&str>,
    ) {
        assert_eq!(
            preset.theme().map(|name| name.as_str().to_string()),
            theme.map(str::to_string)
        );
    }

    #[rstest]
    #[case::cover_style(CoverStyle::Milkdrop, "milkdrop")]
    #[case::speed_chip(SpeedChip::Changed, "changed")]
    #[case::layout_mode(LayoutMode::Compact, "compact")]
    fn display_spells_each_option_the_way_the_file_does(
        #[case] spelled: impl ToString,
        #[case] spelling: &str,
    ) {
        assert_eq!(spelled.to_string(), spelling);
    }
}
