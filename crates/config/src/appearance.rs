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

pub(crate) trait Flag {
    const ON: Self;
    const OFF: Self;
}

macro_rules! flag_enum {
    ($flag:ty, $on:ident, $off:ident) => {
        impl Flag for $flag {
            const ON: Self = Self::$on;
            const OFF: Self = Self::$off;
        }
    };
}

pub(crate) fn flag<'de, D, F>(deserializer: D) -> Result<F, D::Error>
where
    D: Deserializer<'de>,
    F: Flag,
{
    Ok(if bool::deserialize(deserializer)? {
        F::ON
    } else {
        F::OFF
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverBrackets {
    Shown,
    #[default]
    Hidden,
}

flag_enum!(CoverBrackets, Shown, Hidden);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatChips {
    Shown,
    #[default]
    Hidden,
}

flag_enum!(FormatChips, Shown, Hidden);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressTime {
    Remaining,
    #[default]
    Elapsed,
}

flag_enum!(ProgressTime, Remaining, Elapsed);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animations {
    #[default]
    On,
    Off,
}

flag_enum!(Animations, On, Off);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyHints {
    #[default]
    Shown,
    Hidden,
}

flag_enum!(KeyHints, Shown, Hidden);

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

pub(crate) fn preset_appearance(preset: AppearancePreset) -> Appearance {
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

impl From<Appearance> for AppearancePatch {
    fn from(appearance: Appearance) -> Self {
        Self {
            cover_style: Some(appearance.cover_style),
            cover_brackets: Some(appearance.cover_brackets),
            format_chips: Some(appearance.format_chips),
            speed_chip: Some(appearance.speed_chip),
            progress_time: Some(appearance.progress_time),
            key_hints: Some(appearance.key_hints),
            animations: Some(appearance.animations),
            layout_mode: Some(appearance.layout_mode),
        }
    }
}

impl AppearancePatch {
    pub fn apply(self, appearance: Appearance) -> Appearance {
        Appearance {
            cover_style: self.cover_style.unwrap_or(appearance.cover_style),
            cover_brackets: self.cover_brackets.unwrap_or(appearance.cover_brackets),
            format_chips: self.format_chips.unwrap_or(appearance.format_chips),
            speed_chip: self.speed_chip.unwrap_or(appearance.speed_chip),
            progress_time: self.progress_time.unwrap_or(appearance.progress_time),
            key_hints: self.key_hints.unwrap_or(appearance.key_hints),
            animations: self.animations.unwrap_or(appearance.animations),
            layout_mode: self.layout_mode.unwrap_or(appearance.layout_mode),
        }
    }

    pub fn then(self, later: Self) -> Self {
        Self {
            cover_style: later.cover_style.or(self.cover_style),
            cover_brackets: later.cover_brackets.or(self.cover_brackets),
            format_chips: later.format_chips.or(self.format_chips),
            speed_chip: later.speed_chip.or(self.speed_chip),
            progress_time: later.progress_time.or(self.progress_time),
            key_hints: later.key_hints.or(self.key_hints),
            animations: later.animations.or(self.animations),
            layout_mode: later.layout_mode.or(self.layout_mode),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::appearance::{
        Appearance,
        AppearancePatch,
        AppearancePreset,
        CoverStyle,
        FormatChips,
        KeyHints,
        LayoutMode,
        SpeedChip,
        preset_appearance,
        preset_of,
    };

    #[test]
    fn then_folds_disjoint_fields_and_the_later_field_wins() {
        let earlier = AppearancePatch::builder()
            .format_chips(FormatChips::Hidden)
            .cover_style(CoverStyle::Vinyl)
            .build();
        let later = AppearancePatch::builder()
            .cover_style(CoverStyle::Off)
            .key_hints(KeyHints::Hidden)
            .build();

        let merged = earlier.then(later);

        assert_eq!(merged.format_chips, Some(FormatChips::Hidden));
        assert_eq!(merged.key_hints, Some(KeyHints::Hidden));
        assert_eq!(merged.cover_style, Some(CoverStyle::Off));
    }

    #[test]
    fn a_full_patch_applies_to_exactly_the_appearance_it_came_from() {
        let noir = preset_appearance(AppearancePreset::Noir);

        assert_eq!(
            AppearancePatch::from(noir).apply(Appearance::default()),
            noir
        );
    }

    #[test]
    fn the_stock_appearance_is_every_vocabulary_default() {
        insta::assert_debug_snapshot!(Appearance::default());
    }

    #[test]
    fn the_stock_preset_is_exactly_the_stock_appearance() {
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
