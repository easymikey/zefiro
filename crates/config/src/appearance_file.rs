use serde::Deserialize;

use crate::{
    appearance::{
        Animations,
        Appearance,
        AppearancePatch,
        CoverBrackets,
        CoverStyle,
        FormatChips,
        KeyHints,
        LayoutMode,
        ProgressTime,
        SpeedChip,
    },
    error::{Error, TomlFile, parse_toml},
    rgb::Rgb,
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
#[serde(default, deny_unknown_fields)]
pub struct CoverConfig {
    pub size_px: u32,
    pub style: CoverStyle,
    pub text_cells: TextCoverCells,
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub brackets: CoverBrackets,
}

impl Default for CoverConfig {
    fn default() -> Self {
        Self {
            size_px: 160,
            style: CoverStyle::Vinyl,
            text_cells: TextCoverCells::default(),
            brackets: CoverBrackets::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CardConfig {
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub format_chips: FormatChips,
    pub speed_chip: SpeedChip,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProgressConfig {
    pub height_px: f32,
    pub radius: Option<f32>,
    pub fill: Option<Rgb>,
    pub track: Option<Rgb>,
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub remaining: ProgressTime,
}

impl Default for ProgressConfig {
    fn default() -> Self {
        Self {
            height_px: 4.0,
            radius: None,
            fill: None,
            track: None,
            remaining: ProgressTime::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub animations: Animations,
    #[serde(deserialize_with = "crate::appearance::flag")]
    pub key_hints: KeyHints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    pub full_min_width: u16,
    pub full_min_height: u16,
    pub compact_min_width: u16,
    pub compact_min_height: u16,
    pub min_columns: u16,
    pub min_rows: u16,
    pub mode: LayoutMode,
}

impl Default for LayoutConfig {
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
#[serde(default, deny_unknown_fields)]
pub struct AppearanceFile {
    pub card: CardConfig,
    pub progress: ProgressConfig,
    pub cover: CoverConfig,
    pub layout: LayoutConfig,
    pub window: WindowConfig,
}

impl AppearanceFile {
    pub fn appearance(&self) -> Appearance {
        Appearance {
            cover_style: self.cover.style,
            cover_brackets: self.cover.brackets,
            format_chips: self.card.format_chips,
            speed_chip: self.card.speed_chip,
            progress_time: self.progress.remaining,
            key_hints: self.window.key_hints,
            animations: self.window.animations,
            layout_mode: self.layout.mode,
        }
    }

    pub fn with_appearance(self, appearance: Appearance) -> Self {
        Self {
            card: CardConfig {
                format_chips: appearance.format_chips,
                speed_chip: appearance.speed_chip,
            },
            progress: ProgressConfig {
                remaining: appearance.progress_time,
                ..self.progress
            },
            cover: CoverConfig {
                style: appearance.cover_style,
                brackets: appearance.cover_brackets,
                ..self.cover
            },
            layout: LayoutConfig {
                mode: appearance.layout_mode,
                ..self.layout
            },
            window: WindowConfig {
                animations: appearance.animations,
                key_hints: appearance.key_hints,
            },
        }
    }

    pub fn patched(&self, patch: AppearancePatch) -> AppearanceFile {
        self.clone().with_appearance(patch.apply(self.appearance()))
    }
}

pub const APPEARANCE_FILE_NAME: &str = "sifr-ui.toml";

pub fn parse_appearance(source: &str) -> Result<AppearanceFile, Error> {
    parse_toml(source, TomlFile::Appearance)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        appearance::{
            Animations,
            Appearance,
            AppearancePatch,
            AppearancePreset,
            CoverBrackets,
            CoverStyle,
            FormatChips,
            KeyHints,
            LayoutMode,
            ProgressTime,
            SpeedChip,
            preset_appearance,
        },
        appearance_file::{
            AppearanceFile,
            CoverConfig,
            LayoutConfig,
            TextCoverCells,
            parse_appearance,
        },
        error::Error,
    };

    #[test]
    fn the_stock_breakpoints_are_the_documented_defaults() {
        insta::assert_debug_snapshot!(LayoutConfig::default());
    }

    #[test]
    fn the_stock_appearance_file_is_every_tables_defaults() {
        insta::assert_debug_snapshot!(AppearanceFile::default());
    }

    #[test]
    fn the_stock_file_offers_the_stock_appearance() {
        assert_eq!(
            AppearanceFile::default().appearance(),
            Appearance::default()
        );
    }

    #[rstest]
    #[case::stock(Appearance::default())]
    #[case::noir(preset_appearance(AppearancePreset::Noir))]
    fn a_file_written_with_an_appearance_offers_it_back(
        #[case] appearance: Appearance,
    ) {
        assert_eq!(
            AppearanceFile::default()
                .with_appearance(appearance)
                .appearance(),
            appearance
        );
    }

    #[test]
    fn writing_an_appearance_keeps_the_keys_it_says_nothing_about() {
        let sized = AppearanceFile {
            cover: CoverConfig {
                size_px: 320,
                ..CoverConfig::default()
            },
            ..AppearanceFile::default()
        };

        let noir = sized.with_appearance(preset_appearance(AppearancePreset::Noir));

        assert_eq!(noir.cover.size_px, 320);
        assert_eq!(noir.cover.style, CoverStyle::Milkdrop);
    }

    #[test]
    fn an_empty_file_is_all_defaults() {
        assert_eq!(parse_appearance("").unwrap(), AppearanceFile::default());
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
        let source = "[cover]\nstyle = \"plain\"\n[card]\n[card]\n";

        let broken = parse_appearance(source)
            .expect_err("a duplicate table must not parse")
            .to_string();

        assert_eq!(broken.lines().nth(1), Some("sifr-ui.toml:4"), "{broken:?}");
    }

    type Parsed = fn(&AppearanceFile);

    #[rstest]
    #[case::a_plain_cover("[cover]\nstyle = \"plain\"\n", |c: &AppearanceFile| {
        assert_eq!(c.cover.style, CoverStyle::Plain);
    })]
    #[case::a_vinyl_cover("[cover]\nstyle = \"vinyl\"\n", |c: &AppearanceFile| {
        assert_eq!(c.cover.style, CoverStyle::Vinyl);
        assert_eq!(c.cover.size_px, CoverConfig::default().size_px);
    })]
    #[case::a_cover_size("[cover]\nsize_px = 99\n", |c: &AppearanceFile| {
        assert_eq!(c.cover.size_px, 99);
        assert_eq!(c.cover.style, CoverStyle::Vinyl);
    })]
    #[case::a_text_cover_box(
        "[cover]\n[cover.text_cells]\nwidth = 40\nheight = 20\n",
        |c: &AppearanceFile| {
            assert_eq!(c.cover.text_cells, TextCoverCells { width: 40, height: 20 });
            assert_eq!(c.cover.style, CoverStyle::Vinyl);
            assert_eq!(c.cover.size_px, CoverConfig::default().size_px);
        }
    )]
    #[case::every_widget_key(
        "[cover]\nstyle = \"off\"\nbrackets = true\n\
         [card]\nformat_chips = true\nspeed_chip = \"changed\"\n\
         [progress]\nremaining = true\n",
        |c: &AppearanceFile| {
            assert_eq!(c.cover.style, CoverStyle::Off);
            assert_eq!(c.cover.brackets, CoverBrackets::Shown);
            assert_eq!(c.card.format_chips, FormatChips::Shown);
            assert_eq!(c.card.speed_chip, SpeedChip::Changed);
            assert_eq!(c.progress.remaining, ProgressTime::Remaining);
        }
    )]
    #[case::the_window_flags("[window]\nkey_hints = false\n", |c: &AppearanceFile| {
        assert_eq!(c.window.key_hints, KeyHints::Hidden);
    })]
    #[case::a_colour_override("[progress]\nfill = \"#ff0000\"\n", |c: &AppearanceFile| {
        assert_eq!(c.progress.fill.map(|hex| hex.0), Some([255, 0, 0]));
    })]
    #[case::one_breakpoint("[layout]\nfull_min_width = 80\n", |c: &AppearanceFile| {
        let stock = LayoutConfig::default();
        assert_eq!(c.layout.full_min_width, 80);
        assert_eq!(c.layout.full_min_height, stock.full_min_height);
        assert_eq!(c.layout.compact_min_width, stock.compact_min_width);
        assert_eq!(c.layout.compact_min_height, stock.compact_min_height);
    })]
    #[case::a_layout_mode("[layout]\nmode = \"compact\"\n", |c: &AppearanceFile| {
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
    #[case::a_removed_notice_table("[notice]\nstyle = \"banner\"\n")]
    #[case::a_removed_theme_key("theme = \"oreo\"\n")]
    #[case::a_removed_keymap_table("[keymap]\nnext = \"x\"\n")]
    #[case::a_misspelt_key("[cover]\nbrakcets = true\n")]
    fn a_key_nothing_reads_is_rejected(#[case] text: &str) {
        assert!(parse_appearance(text).is_err(), "{text} must not parse");
    }

    #[rstest]
    #[case::cover_style(
        AppearancePatch::builder().cover_style(CoverStyle::Milkdrop).build()
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
        let base = AppearanceFile::default();
        let after = base.patched(patch).appearance();
        let expected = Appearance {
            cover_style: patch
                .cover_style
                .unwrap_or(Appearance::default().cover_style),
            cover_brackets: patch
                .cover_brackets
                .unwrap_or(Appearance::default().cover_brackets),
            format_chips: patch
                .format_chips
                .unwrap_or(Appearance::default().format_chips),
            speed_chip: patch.speed_chip.unwrap_or(Appearance::default().speed_chip),
            progress_time: patch
                .progress_time
                .unwrap_or(Appearance::default().progress_time),
            key_hints: patch.key_hints.unwrap_or(Appearance::default().key_hints),
            animations: patch.animations.unwrap_or(Appearance::default().animations),
            layout_mode: patch
                .layout_mode
                .unwrap_or(Appearance::default().layout_mode),
        };
        assert_eq!(after, expected);
    }

    #[test]
    fn patched_with_an_empty_patch_leaves_every_option_untouched() {
        let base = AppearanceFile::default()
            .with_appearance(preset_appearance(AppearancePreset::Noir));
        let after = base.patched(AppearancePatch::builder().build());
        assert_eq!(after, base);
    }

    #[test]
    fn patched_keeps_the_keys_the_patch_says_nothing_about() {
        let sized = AppearanceFile {
            cover: CoverConfig {
                size_px: 320,
                ..CoverConfig::default()
            },
            ..AppearanceFile::default()
        };

        let after = sized.patched(
            AppearancePatch::builder()
                .cover_style(CoverStyle::Off)
                .build(),
        );

        assert_eq!(after.cover.size_px, 320);
        assert_eq!(after.cover.style, CoverStyle::Off);
    }
}
