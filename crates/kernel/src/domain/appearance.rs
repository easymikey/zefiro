use std::{fmt, str::FromStr};

use strum::{EnumIter, EnumString, IntoEnumIterator, VariantNames};

use crate::domain::{
    geometry::{Cells, Pixels},
    theme::ThemeName,
};

pub const DEFAULT_COVER_WIDTH: Cells = Cells(20);
pub const DEFAULT_COVER_HEIGHT: Cells = Cells(8);
pub const DEFAULT_FULL_MIN_WIDTH: Cells = Cells(60);
pub const DEFAULT_FULL_MIN_HEIGHT: Cells = Cells(19);
pub const DEFAULT_COMPACT_MIN_WIDTH: Cells = Cells(30);
pub const DEFAULT_COMPACT_MIN_HEIGHT: Cells = Cells(13);
pub const DEFAULT_MIN_WIDTH: Cells = Cells(48);
pub const DEFAULT_MIN_HEIGHT: Cells = Cells(16);

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Appearance {
    pub cover_cells: CoverCells,
    pub breakpoints: Breakpoints,
    pub progress_bar: ProgressBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverCells {
    pub width: Cells,
    pub height: Cells,
}

impl Default for CoverCells {
    fn default() -> Self {
        Self {
            width: DEFAULT_COVER_WIDTH,
            height: DEFAULT_COVER_HEIGHT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakpoints {
    pub full_min_width: Cells,
    pub full_min_height: Cells,
    pub compact_min_width: Cells,
    pub compact_min_height: Cells,
    pub min_width: Cells,
    pub min_height: Cells,
}

impl Default for Breakpoints {
    fn default() -> Self {
        Self {
            full_min_width: DEFAULT_FULL_MIN_WIDTH,
            full_min_height: DEFAULT_FULL_MIN_HEIGHT,
            compact_min_width: DEFAULT_COMPACT_MIN_WIDTH,
            compact_min_height: DEFAULT_COMPACT_MIN_HEIGHT,
            min_width: DEFAULT_MIN_WIDTH,
            min_height: DEFAULT_MIN_HEIGHT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBar {
    pub height: Pixels,
    pub radius: Option<Pixels>,
    pub fill: Option<Rgb>,
    pub groove: Option<Rgb>,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            height: Pixels(4),
            radius: None,
            fill: None,
            groove: None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, strum::Display, EnumString, VariantNames,
)]
#[strum(serialize_all = "lowercase")]
pub enum CoverMode {
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
pub struct AppearanceSettings {
    pub cover_mode: CoverMode,
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

pub fn preset_appearance(preset: AppearancePreset) -> AppearanceSettings {
    match preset {
        AppearancePreset::Stock => AppearanceSettings::default(),
        AppearancePreset::Noir => AppearanceSettings {
            cover_mode: CoverMode::Milkdrop,
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
pub fn preset_of(appearance_settings: AppearanceSettings) -> Option<AppearancePreset> {
    AppearancePreset::iter()
        .find(|&preset| preset_appearance(preset) == appearance_settings)
}

#[must_use]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppearancePatch {
    pub cover_mode: Option<CoverMode>,
    pub cover_brackets: Option<CoverBrackets>,
    pub format_chips: Option<FormatChips>,
    pub speed_chip: Option<SpeedChip>,
    pub progress_time: Option<ProgressTime>,
    pub key_hints: Option<KeyHints>,
    pub animations: Option<Animations>,
    pub layout_mode: Option<LayoutMode>,
}

impl From<AppearanceSettings> for AppearancePatch {
    fn from(appearance_settings: AppearanceSettings) -> Self {
        Self {
            cover_mode: Some(appearance_settings.cover_mode),
            cover_brackets: Some(appearance_settings.cover_brackets),
            format_chips: Some(appearance_settings.format_chips),
            speed_chip: Some(appearance_settings.speed_chip),
            progress_time: Some(appearance_settings.progress_time),
            key_hints: Some(appearance_settings.key_hints),
            animations: Some(appearance_settings.animations),
            layout_mode: Some(appearance_settings.layout_mode),
        }
    }
}

impl AppearanceSettings {
    pub fn patched(self, patch: AppearancePatch) -> AppearanceSettings {
        AppearanceSettings {
            cover_mode: patch.cover_mode.unwrap_or(self.cover_mode),
            cover_brackets: patch.cover_brackets.unwrap_or(self.cover_brackets),
            format_chips: patch.format_chips.unwrap_or(self.format_chips),
            speed_chip: patch.speed_chip.unwrap_or(self.speed_chip),
            progress_time: patch.progress_time.unwrap_or(self.progress_time),
            key_hints: patch.key_hints.unwrap_or(self.key_hints),
            animations: patch.animations.unwrap_or(self.animations),
            layout_mode: patch.layout_mode.unwrap_or(self.layout_mode),
        }
    }
}

impl AppearancePatch {
    pub fn then(self, later: Self) -> Self {
        Self {
            cover_mode: later.cover_mode.or(self.cover_mode),
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
#[error("invalid color `{0}`: expected 6 hex digits as #rrggbb")]
struct HexError(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColorError {
    #[error("{0}")]
    Malformed(crate::domain::config::Diagnostic),
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
            .ok_or_else(|| {
                ColorError::Malformed(crate::domain::config::Diagnostic::from_error(
                    &HexError(spelling.into()),
                ))
            })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(formatter, "#{r:02x}{g:02x}{b:02x}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::appearance::{
        AppearancePatch,
        AppearancePreset,
        AppearanceSettings,
        CoverMode,
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
        let earlier_patch = AppearancePatch {
            format_chips: Some(FormatChips::Hidden),
            cover_mode: Some(CoverMode::Vinyl),
            ..AppearancePatch::default()
        };
        let later = AppearancePatch {
            cover_mode: Some(CoverMode::Off),
            key_hints: Some(KeyHints::Hidden),
            ..AppearancePatch::default()
        };

        let merged = earlier_patch.then(later);

        assert_eq!(merged.format_chips, Some(FormatChips::Hidden));
        assert_eq!(merged.key_hints, Some(KeyHints::Hidden));
        assert_eq!(merged.cover_mode, Some(CoverMode::Off));
    }

    #[test]
    fn a_full_patch_applies_to_exactly_the_appearance_it_came_from() {
        let noir = preset_appearance(AppearancePreset::Noir);

        assert_eq!(
            AppearanceSettings::default().patched(AppearancePatch::from(noir)),
            noir
        );
    }

    #[test]
    fn the_stock_appearance_is_every_vocabulary_default() {
        insta::assert_debug_snapshot!(AppearanceSettings::default());
    }

    #[test]
    fn the_stock_preset_is_exactly_the_stock_appearance() {
        assert_eq!(
            preset_appearance(AppearancePreset::Stock),
            AppearanceSettings::default()
        );
    }

    #[test]
    fn the_noir_preset_names_every_option_it_changes() {
        insta::assert_debug_snapshot!(preset_appearance(AppearancePreset::Noir));
    }

    #[rstest]
    #[case::stock(AppearanceSettings::default(), Some(AppearancePreset::Stock))]
    #[case::noir(
        preset_appearance(AppearancePreset::Noir),
        Some(AppearancePreset::Noir)
    )]
    #[case::a_custom_mix(
        AppearanceSettings { format_chips: FormatChips::Shown, ..AppearanceSettings::default() },
        None
    )]
    fn preset_of_names_the_preset_an_appearance_came_from(
        #[case] appearance_settings: AppearanceSettings,
        #[case] preset: Option<AppearancePreset>,
    ) {
        assert_eq!(preset_of(appearance_settings), preset);
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
    #[case::cover_mode(CoverMode::Milkdrop, "milkdrop")]
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
