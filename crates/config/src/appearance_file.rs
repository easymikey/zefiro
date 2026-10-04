use kernel::domain::{
    appearance::{
        Animations,
        AppearancePatch,
        AppearanceSettings,
        CoverBrackets,
        CoverMode,
        FormatChips,
        KeyHints,
        LayoutMode,
        ProgressTime,
        Rgb,
        SpeedChip,
    },
    config::ConfigName,
    geometry::Pixels,
};
use serde::Deserialize;

use crate::{
    appearance::{from_str_option, rounded_pixels, variant_field},
    error::{Error, parse_toml},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextCoverCells {
    pub width: u16,
    pub height: u16,
}

impl Default for TextCoverCells {
    fn default() -> Self {
        Self {
            width: 20,
            height: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "a [cover] table")]
pub struct TomlCover {
    #[serde(deserialize_with = "variant_field")]
    pub(crate) mode: CoverMode,
    pub text_cells: TextCoverCells,
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub(crate) brackets: CoverBrackets,
}

impl Default for TomlCover {
    fn default() -> Self {
        Self {
            mode: CoverMode::Vinyl,
            text_cells: TextCoverCells::default(),
            brackets: CoverBrackets::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "a [card] table")]
pub struct TomlCard {
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub(crate) format_chips: FormatChips,
    #[serde(deserialize_with = "variant_field")]
    pub(crate) speed_chip: SpeedChip,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "a [progress] table")]
pub struct TomlProgress {
    #[serde(deserialize_with = "rounded_pixels")]
    pub height_px: Pixels,
    #[serde(deserialize_with = "rounded_pixels")]
    pub radius: Option<Pixels>,
    #[serde(deserialize_with = "from_str_option")]
    pub fill: Option<Rgb>,
    #[serde(deserialize_with = "from_str_option")]
    pub groove: Option<Rgb>,
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub(crate) remaining: ProgressTime,
}

impl Default for TomlProgress {
    fn default() -> Self {
        Self {
            height_px: Pixels(4),
            radius: None,
            fill: None,
            groove: None,
            remaining: ProgressTime::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "a [window] table")]
pub struct TomlWindow {
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub(crate) animations: Animations,
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub(crate) key_hints: KeyHints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "a [layout] table")]
pub struct TomlLayout {
    pub full_min_width: u16,
    pub full_min_height: u16,
    pub compact_min_width: u16,
    pub compact_min_height: u16,
    pub min_columns: u16,
    pub min_rows: u16,
    #[serde(deserialize_with = "variant_field")]
    pub(crate) mode: LayoutMode,
}

impl Default for TomlLayout {
    fn default() -> Self {
        Self {
            full_min_width: 60,
            full_min_height: 19,
            compact_min_width: 30,
            compact_min_height: 13,
            min_columns: 48,
            min_rows: 16,
            mode: LayoutMode::default(),
        }
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "the sifr-ui.toml file")]
pub struct TomlAppearance {
    pub(crate) card: TomlCard,
    pub progress: TomlProgress,
    pub cover: TomlCover,
    pub layout: TomlLayout,
    pub(crate) window: TomlWindow,
}

impl TomlAppearance {
    pub fn settings(&self) -> AppearanceSettings {
        AppearanceSettings {
            cover_mode: self.cover.mode,
            cover_brackets: self.cover.brackets,
            format_chips: self.card.format_chips,
            speed_chip: self.card.speed_chip,
            progress_time: self.progress.remaining,
            key_hints: self.window.key_hints,
            animations: self.window.animations,
            layout_mode: self.layout.mode,
        }
    }

    pub fn with_appearance(self, appearance: AppearanceSettings) -> Self {
        Self {
            card: TomlCard {
                format_chips: appearance.format_chips,
                speed_chip: appearance.speed_chip,
            },
            progress: TomlProgress {
                remaining: appearance.progress_time,
                ..self.progress
            },
            cover: TomlCover {
                mode: appearance.cover_mode,
                brackets: appearance.cover_brackets,
                ..self.cover
            },
            layout: TomlLayout {
                mode: appearance.layout_mode,
                ..self.layout
            },
            window: TomlWindow {
                animations: appearance.animations,
                key_hints: appearance.key_hints,
            },
        }
    }

    pub fn patched(&self, patch: AppearancePatch) -> TomlAppearance {
        self.clone().with_appearance(self.settings().patched(patch))
    }
}

pub const APPEARANCE_FILE_NAME: &str = "sifr-ui.toml";

pub fn parse_appearance(source: &str) -> Result<TomlAppearance, Error> {
    parse_toml(source, ConfigName::Appearance)
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::{
            Animations,
            AppearancePatch,
            AppearancePreset,
            AppearanceSettings,
            CoverBrackets,
            CoverMode,
            FormatChips,
            KeyHints,
            LayoutMode,
            ProgressTime,
            SpeedChip,
            preset_appearance,
        },
        geometry::Pixels,
    };
    use rstest::rstest;

    use crate::{
        appearance_file::{
            TextCoverCells,
            TomlAppearance,
            TomlLayout,
            TomlProgress,
            parse_appearance,
        },
        error::Error,
    };

    #[test]
    fn the_stock_breakpoints_are_the_documented_defaults() {
        insta::assert_debug_snapshot!(TomlLayout::default());
    }

    #[test]
    fn the_stock_appearance_file_is_every_tables_defaults() {
        insta::assert_debug_snapshot!(TomlAppearance::default());
    }

    #[test]
    fn the_stock_file_offers_the_stock_appearance() {
        assert_eq!(
            TomlAppearance::default().settings(),
            AppearanceSettings::default()
        );
    }

    #[test]
    fn an_unknown_variant_lists_the_ones_that_exist() {
        let error = parse_appearance("[cover]\nmode = \"bogus\"\n")
            .expect_err("an unknown mode must not parse")
            .to_string();

        assert!(
            error.contains(
                "unknown variant `bogus`, expected one of `vinyl`, `plain`, `milkdrop`, `off`"
            ),
            "{error}"
        );
    }

    #[rstest]
    #[case::stock(AppearanceSettings::default())]
    #[case::noir(preset_appearance(AppearancePreset::Noir))]
    fn a_file_written_with_an_appearance_offers_it_back(
        #[case] appearance: AppearanceSettings,
    ) {
        assert_eq!(
            TomlAppearance::default()
                .with_appearance(appearance)
                .settings(),
            appearance
        );
    }

    #[test]
    fn writing_an_appearance_keeps_the_keys_it_says_nothing_about() {
        let sized = TomlAppearance {
            progress: TomlProgress {
                height_px: Pixels(9),
                ..TomlProgress::default()
            },
            ..TomlAppearance::default()
        };

        let noir = sized.with_appearance(preset_appearance(AppearancePreset::Noir));

        assert_eq!(noir.progress.height_px, Pixels(9));
        assert_eq!(noir.cover.mode, CoverMode::Milkdrop);
    }

    #[test]
    fn an_empty_file_is_all_defaults() {
        assert_eq!(parse_appearance("").unwrap(), TomlAppearance::default());
    }

    #[test]
    fn a_broken_file_reports_the_real_parse_error() {
        assert!(matches!(
            parse_appearance("[cover\nnot toml"),
            Err(Error::Parse { .. })
        ));
    }

    #[rstest]
    #[case::an_unknown_top_level_table("unknown_table", "[nope]\nkey = 1\n")]
    #[case::an_unknown_key_in_a_known_table("unknown_key", "[cover]\nbogus = 1\n")]
    fn unknown_toml_names_the_key(#[case] name: &str, #[case] text: &str) {
        let error = parse_appearance(text).expect_err("unknown TOML must not parse");
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(error.to_string());
        });
    }

    #[test]
    fn a_duplicate_table_names_the_ui_file_and_its_line() {
        let source = "[cover]\nmode = \"plain\"\n[card]\n[card]\n";

        let broken = parse_appearance(source)
            .expect_err("a duplicate table must not parse")
            .to_string();

        assert_eq!(broken.lines().nth(1), Some("sifr-ui.toml:4"), "{broken:?}");
    }

    type Parsed = fn(&TomlAppearance);

    #[rstest]
    #[case::a_plain_cover("[cover]\nmode = \"plain\"\n", |c: &TomlAppearance| {
        assert_eq!(c.cover.mode, CoverMode::Plain);
    })]
    #[case::a_vinyl_cover("[cover]\nmode = \"vinyl\"\n", |c: &TomlAppearance| {
        assert_eq!(c.cover.mode, CoverMode::Vinyl);
    })]
    #[case::a_fractional_bar("[progress]\nheight_px = 5.6\nradius = 2.4\n", |c: &TomlAppearance| {
        assert_eq!(c.progress.height_px, Pixels(6));
        assert_eq!(c.progress.radius, Some(Pixels(2)));
    })]
    #[case::a_whole_bar("[progress]\nheight_px = 7\n", |c: &TomlAppearance| {
        assert_eq!(c.progress.height_px, Pixels(7));
        assert_eq!(c.progress.radius, None);
    })]
    #[case::a_text_cover_box(
        "[cover]\n[cover.text_cells]\nwidth = 40\nheight = 20\n",
        |c: &TomlAppearance| {
            assert_eq!(c.cover.text_cells, TextCoverCells { width: 40, height: 20 });
            assert_eq!(c.cover.mode, CoverMode::Vinyl);
        }
    )]
    #[case::every_widget_key(
        "[cover]\nmode = \"off\"\nbrackets = true\n\
         [card]\nformat_chips = true\nspeed_chip = \"changed\"\n\
         [progress]\nremaining = true\n",
        |c: &TomlAppearance| {
            assert_eq!(c.cover.mode, CoverMode::Off);
            assert_eq!(c.cover.brackets, CoverBrackets::Shown);
            assert_eq!(c.card.format_chips, FormatChips::Shown);
            assert_eq!(c.card.speed_chip, SpeedChip::Changed);
            assert_eq!(c.progress.remaining, ProgressTime::Remaining);
        }
    )]
    #[case::the_window_flags("[window]\nkey_hints = false\n", |c: &TomlAppearance| {
        assert_eq!(c.window.key_hints, KeyHints::Hidden);
    })]
    #[case::a_colour_override("[progress]\nfill = \"#ff0000\"\n", |c: &TomlAppearance| {
        assert_eq!(c.progress.fill.map(|hex| hex.0), Some([255, 0, 0]));
    })]
    #[case::one_breakpoint("[layout]\nfull_min_width = 80\n", |c: &TomlAppearance| {
        let stock = TomlLayout::default();
        assert_eq!(c.layout.full_min_width, 80);
        assert_eq!(c.layout.full_min_height, stock.full_min_height);
        assert_eq!(c.layout.compact_min_width, stock.compact_min_width);
        assert_eq!(c.layout.compact_min_height, stock.compact_min_height);
    })]
    #[case::a_layout_mode("[layout]\nmode = \"compact\"\n", |c: &TomlAppearance| {
        assert_eq!(c.layout.mode, LayoutMode::Compact);
    })]
    fn parse_appearance_reads_each_key_fieldwise(
        #[case] text: &str,
        #[case] parsed: Parsed,
    ) {
        parsed(&parse_appearance(text).unwrap());
    }

    #[rstest]
    #[case::a_removed_cover_mode("[cover]\nmode = \"text\"\n")]
    #[case::a_removed_progress_mode("[progress]\nmode = \"pixel\"\n")]
    #[case::a_removed_volume_table("[volume]\nmode = \"text\"\n")]
    #[case::a_removed_notice_table("[notice]\nmode = \"banner\"\n")]
    #[case::a_removed_theme_key("theme = \"oreo\"\n")]
    #[case::a_removed_keymap_table("[keymap]\nnext = \"x\"\n")]
    #[case::a_misspelt_key("[cover]\nbrakcets = true\n")]
    #[case::a_negative_bar_height("[progress]\nheight_px = -1\n")]
    fn a_key_nothing_reads_is_rejected(#[case] text: &str) {
        assert!(parse_appearance(text).is_err(), "{text} must not parse");
    }

    #[rstest]
    #[case::cover_mode(
        AppearancePatch::builder().cover_mode(CoverMode::Milkdrop).build()
    )]
    #[case::cover_brackets(
        AppearancePatch::builder().cover_brackets(CoverBrackets::Shown).build()
    )]
    #[case::format_chips(
        AppearancePatch::builder().format_chips(FormatChips::Shown).build()
    )]
    #[case::speed_chip(
        AppearancePatch::builder().speed_chip(SpeedChip::Never).build()
    )]
    #[case::progress_time(
        AppearancePatch::builder().progress_time(ProgressTime::Remaining).build()
    )]
    #[case::key_hints(
        AppearancePatch::builder().key_hints(KeyHints::Hidden).build()
    )]
    #[case::animations(
        AppearancePatch::builder().animations(Animations::Off).build()
    )]
    #[case::layout_mode(
        AppearancePatch::builder().layout_mode(LayoutMode::Compact).build()
    )]
    fn patched_applies_exactly_the_row_the_patch_names(#[case] patch: AppearancePatch) {
        let base = TomlAppearance::default();
        let after = base.patched(patch).settings();
        let expected = AppearanceSettings {
            cover_mode: patch
                .cover_mode
                .unwrap_or(AppearanceSettings::default().cover_mode),
            cover_brackets: patch
                .cover_brackets
                .unwrap_or(AppearanceSettings::default().cover_brackets),
            format_chips: patch
                .format_chips
                .unwrap_or(AppearanceSettings::default().format_chips),
            speed_chip: patch
                .speed_chip
                .unwrap_or(AppearanceSettings::default().speed_chip),
            progress_time: patch
                .progress_time
                .unwrap_or(AppearanceSettings::default().progress_time),
            key_hints: patch
                .key_hints
                .unwrap_or(AppearanceSettings::default().key_hints),
            animations: patch
                .animations
                .unwrap_or(AppearanceSettings::default().animations),
            layout_mode: patch
                .layout_mode
                .unwrap_or(AppearanceSettings::default().layout_mode),
        };
        assert_eq!(after, expected);
    }

    #[test]
    fn patched_with_an_empty_patch_leaves_every_option_untouched() {
        let base = TomlAppearance::default()
            .with_appearance(preset_appearance(AppearancePreset::Noir));
        let after = base.patched(AppearancePatch::builder().build());
        assert_eq!(after, base);
    }

    #[test]
    fn patched_keeps_the_keys_the_patch_says_nothing_about() {
        let sized = TomlAppearance {
            progress: TomlProgress {
                height_px: Pixels(9),
                ..TomlProgress::default()
            },
            ..TomlAppearance::default()
        };

        let after = sized.patched(
            AppearancePatch::builder()
                .cover_mode(CoverMode::Off)
                .build(),
        );

        assert_eq!(after.progress.height_px, Pixels(9));
        assert_eq!(after.cover.mode, CoverMode::Off);
    }
}
