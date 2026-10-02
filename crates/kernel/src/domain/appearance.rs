use std::{fmt, str::FromStr};

use strum::{EnumIter, EnumString, IntoEnumIterator, VariantNames};

use crate::domain::ThemeName;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, EnumString, VariantNames,
)]
#[strum(serialize_all = "lowercase")]
pub enum CoverStyle {
    #[default]
    Vinyl,
    Plain,
    Milkdrop,
    Off,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, EnumString, VariantNames,
)]
#[strum(serialize_all = "lowercase")]
pub enum SpeedChip {
    #[default]
    Always,
    Changed,
    Never,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, EnumString, VariantNames,
)]
#[strum(serialize_all = "lowercase")]
pub enum LayoutMode {
    #[default]
    Auto,
    Full,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverBrackets {
    Shown,
    #[default]
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatChips {
    Shown,
    #[default]
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressTime {
    Remaining,
    #[default]
    Elapsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animations {
    #[default]
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyHints {
    #[default]
    Shown,
    Hidden,
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid color `{input}`: expected 6 hex digits as #rrggbb")]
pub struct ColorError {
    pub input: String,
}

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte.to_ascii_lowercase() {
        digit @ b'0'..=b'9' => Some(digit - b'0'),
        letter @ b'a'..=b'f' => Some(letter - b'a' + 10),
        _ => None,
    }
}

fn hex_byte(high: u8, low: u8) -> Option<u8> {
    Some(hex_nibble(high)? * 16 + hex_nibble(low)?)
}

impl FromStr for Rgb {
    type Err = ColorError;

    fn from_str(spelling: &str) -> Result<Self, Self::Err> {
        let trimmed = spelling.strip_prefix('#').unwrap_or(spelling);
        let sized = <[u8; 6]>::try_from(trimmed.as_bytes()).ok();
        sized
            .and_then(|[r1, r0, g1, g0, b1, b0]| {
                Some(Rgb([
                    hex_byte(r1, r0)?,
                    hex_byte(g1, g0)?,
                    hex_byte(b1, b0)?,
                ]))
            })
            .ok_or_else(|| ColorError {
                input: spelling.to_string(),
            })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(formatter, "#{r:02x}{g:02x}{b:02x}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverCells {
    pub width: u16,
    pub height: u16,
}

impl Default for CoverCells {
    fn default() -> Self {
        Self {
            width: 20,
            height: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakpoints {
    pub full_min_width: u16,
    pub full_min_height: u16,
    pub compact_min_width: u16,
    pub compact_min_height: u16,
    pub min_columns: u16,
    pub min_rows: u16,
}

impl Default for Breakpoints {
    fn default() -> Self {
        Self {
            full_min_width: 60,
            full_min_height: 19,
            compact_min_width: 30,
            compact_min_height: 13,
            min_columns: 48,
            min_rows: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBar {
    pub height_px: f32,
    pub radius: Option<f32>,
    pub fill: Option<Rgb>,
    pub track: Option<Rgb>,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            height_px: 4.0,
            radius: None,
            fill: None,
            track: None,
        }
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub appearance: Appearance,
    pub cover_size_px: u32,
    pub cover_cells: CoverCells,
    pub breakpoints: Breakpoints,
    pub progress: ProgressBar,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            cover_size_px: 160,
            cover_cells: CoverCells::default(),
            breakpoints: Breakpoints::default(),
            progress: ProgressBar::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::appearance::{
        Appearance,
        AppearancePatch,
        AppearancePreset,
        CoverStyle,
        FormatChips,
        KeyHints,
        LayoutMode,
        Rgb,
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

    #[rstest]
    #[case::with_hash("#2aa8a0", [0x2a, 0xa8, 0xa0])]
    #[case::without_hash("2aa8a0", [0x2a, 0xa8, 0xa0])]
    fn parse_accepts_valid_six_digit_hex(#[case] input: &str, #[case] want: [u8; 3]) {
        match input.parse::<Rgb>() {
            Ok(hex) => assert_eq!(hex, Rgb(want)),
            Err(error) => panic!("expected {input:?} to parse, got {error}"),
        }
    }

    #[rstest]
    #[case::too_short("#fff")]
    #[case::too_long("#ffffffff")]
    #[case::empty("")]
    #[case::non_hex_digit("#ff00zz")]
    fn parse_rejects_invalid_input(#[case] input: &str) {
        assert!(input.parse::<Rgb>().is_err());
    }

    #[test]
    fn display_spells_the_hex_back_with_a_hash_and_lowercase_digits() {
        assert_eq!(Rgb([0x2a, 0xa8, 0xf0]).to_string(), "#2aa8f0");
    }
}
