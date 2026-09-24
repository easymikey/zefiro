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
pub enum SpeedChipMode {
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

pub(crate) fn two_state<'de, D, Flag>(
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
    two_state(deserializer, CoverBrackets::Shown, CoverBrackets::Hidden)
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
    two_state(deserializer, FormatChips::Shown, FormatChips::Hidden)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressStyle {
    Remaining,
    #[default]
    Elapsed,
}

pub(crate) fn progress_style<'de, D>(deserializer: D) -> Result<ProgressStyle, D::Error>
where
    D: Deserializer<'de>,
{
    two_state(
        deserializer,
        ProgressStyle::Remaining,
        ProgressStyle::Elapsed,
    )
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
    two_state(deserializer, Animations::On, Animations::Off)
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
    two_state(deserializer, KeyHints::Shown, KeyHints::Hidden)
}

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Appearance {
    pub cover_style: CoverStyle,
    pub cover_brackets: CoverBrackets,
    pub format_chips: FormatChips,
    pub speed_chip: SpeedChipMode,
    pub progress_remaining: ProgressStyle,
    pub key_hints: KeyHints,
    pub animations: Animations,
    pub layout_mode: LayoutMode,
}

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, EnumIter)]
pub enum AppearancePreset {
    #[default]
    Default,
    Noir,
}

impl AppearancePreset {
    #[must_use]
    pub const fn theme(self) -> Option<&'static str> {
        match self {
            AppearancePreset::Default => None,
            AppearancePreset::Noir => Some("noir"),
        }
    }
}

pub fn preset_options(preset: AppearancePreset) -> Appearance {
    match preset {
        AppearancePreset::Default => Appearance::default(),
        AppearancePreset::Noir => Appearance {
            cover_style: CoverStyle::Milkdrop,
            cover_brackets: CoverBrackets::Shown,
            format_chips: FormatChips::Shown,
            speed_chip: SpeedChipMode::Always,
            progress_remaining: ProgressStyle::Remaining,
            key_hints: KeyHints::Shown,
            animations: Animations::On,
            layout_mode: LayoutMode::Auto,
        },
    }
}

#[must_use]
pub fn preset_of(appearance: Appearance) -> Option<AppearancePreset> {
    AppearancePreset::iter().find(|&preset| preset_options(preset) == appearance)
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
    pub speed_chip: Option<SpeedChipMode>,
    #[builder(setters(option_fn(name = with_progress_remaining)))]
    pub progress_remaining: Option<ProgressStyle>,
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
        SpeedChipMode,
        preset_of,
        preset_options,
    };

    #[test]
    fn the_stock_appearance_is_every_vocabulary_default() {
        insta::assert_debug_snapshot!(Appearance::default());
    }

    #[test]
    fn the_default_preset_is_exactly_the_stock_appearance() {
        assert_eq!(
            preset_options(AppearancePreset::Default),
            Appearance::default()
        );
    }

    #[test]
    fn the_noir_preset_names_every_option_it_changes() {
        insta::assert_debug_snapshot!(preset_options(AppearancePreset::Noir));
    }

    #[rstest]
    #[case::stock(Appearance::default(), Some(AppearancePreset::Default))]
    #[case::noir(preset_options(AppearancePreset::Noir), Some(AppearancePreset::Noir))]
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
    #[case(AppearancePreset::Default, None)]
    #[case(AppearancePreset::Noir, Some("noir"))]
    fn a_preset_names_the_theme_it_wants(
        #[case] preset: AppearancePreset,
        #[case] theme: Option<&str>,
    ) {
        assert_eq!(preset.theme(), theme);
    }

    #[rstest]
    #[case::cover_style(CoverStyle::Milkdrop, "milkdrop")]
    #[case::speed_chip(SpeedChipMode::Changed, "changed")]
    #[case::layout_mode(LayoutMode::Compact, "compact")]
    fn display_spells_each_option_the_way_the_file_does(
        #[case] spelled: impl ToString,
        #[case] spelling: &str,
    ) {
        assert_eq!(spelled.to_string(), spelling);
    }
}
