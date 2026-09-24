use toml_edit::{DocumentMut, value};

use crate::{
    appearance::{
        Animations,
        AppearancePatch,
        CoverBrackets,
        CoverStyle,
        FormatChips,
        KeyHints,
        LayoutMode,
        ProgressStyle,
        SpeedChipMode,
    },
    document::{Field, write_fields},
    error::ConfigError,
};

fn cover_fields(
    style: Option<CoverStyle>,
    brackets: Option<CoverBrackets>,
) -> [Field; 2] {
    [
        (
            "cover",
            "style",
            style.map(|style| value(style.to_string())),
        ),
        (
            "cover",
            "brackets",
            brackets.map(|brackets| value(matches!(brackets, CoverBrackets::Shown))),
        ),
    ]
}

fn card_fields(
    format_chips: Option<FormatChips>,
    speed_chip: Option<SpeedChipMode>,
) -> [Field; 2] {
    [
        (
            "card",
            "format_chips",
            format_chips.map(|chips| value(matches!(chips, FormatChips::Shown))),
        ),
        (
            "card",
            "speed_chip",
            speed_chip.map(|speed| value(speed.to_string())),
        ),
    ]
}

fn progress_fields(remaining: Option<ProgressStyle>) -> [Field; 1] {
    [(
        "progress",
        "remaining",
        remaining.map(|style| value(matches!(style, ProgressStyle::Remaining))),
    )]
}

#[derive(Debug, Clone, Copy)]
struct WindowFields {
    key_hints: Option<KeyHints>,
    animations: Option<Animations>,
}

fn window_fields(fields: WindowFields) -> [Field; 2] {
    let WindowFields {
        key_hints,
        animations,
    } = fields;
    [
        (
            "window",
            "key_hints",
            key_hints.map(|key_hints| value(matches!(key_hints, KeyHints::Shown))),
        ),
        (
            "window",
            "animations",
            animations.map(|animations| value(matches!(animations, Animations::On))),
        ),
    ]
}

fn layout_fields(mode: Option<LayoutMode>) -> [Field; 1] {
    [("layout", "mode", mode.map(|mode| value(mode.to_string())))]
}

fn patch_appearance(
    doc: &mut DocumentMut,
    patch: AppearancePatch,
) -> Result<(), ConfigError> {
    let AppearancePatch {
        cover_style,
        cover_brackets,
        format_chips,
        speed_chip,
        progress_remaining,
        key_hints,
        animations,
        layout_mode,
    } = patch;
    write_fields(doc, cover_fields(cover_style, cover_brackets))?;
    write_fields(doc, card_fields(format_chips, speed_chip))?;
    write_fields(doc, progress_fields(progress_remaining))?;
    write_fields(
        doc,
        window_fields(WindowFields {
            key_hints,
            animations,
        }),
    )?;
    write_fields(doc, layout_fields(layout_mode))
}

pub fn appearance_patched(
    text: &str,
    patch: AppearancePatch,
) -> Result<String, ConfigError> {
    let mut doc: DocumentMut = text.parse()?;
    patch_appearance(&mut doc, patch)?;
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

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
            ProgressStyle,
            SpeedChipMode,
        },
        appearance_document::appearance_patched,
        appearance_file::parse_appearance,
        error::ConfigError,
    };

    const COMMENTED_UI: &str = include_str!("../tests/fixtures/sifr-ui_commented.toml");

    fn every_appearance_field_in_text() -> AppearancePatch {
        AppearancePatch {
            cover_style: Some(CoverStyle::Milkdrop),
            cover_brackets: Some(CoverBrackets::Shown),
            format_chips: Some(FormatChips::Shown),
            speed_chip: Some(SpeedChipMode::Always),
            progress_remaining: Some(ProgressStyle::Remaining),
            key_hints: Some(KeyHints::Hidden),
            animations: Some(Animations::Off),
            layout_mode: Some(LayoutMode::Compact),
        }
    }

    #[rstest]
    #[case::an_empty_patch_changes_nothing(
        "empty_patch",
        COMMENTED_UI,
        AppearancePatch::builder().build()
    )]
    #[case::minimal_sections_on_an_empty_document(
        "minimal_sections",
        "",
        AppearancePatch::builder()
            .cover_style(CoverStyle::Off)
            .format_chips(FormatChips::Shown)
            .build()
    )]
    #[case::the_key_hints_lands_in_the_window_table(
        "key_hints",
        "",
        AppearancePatch::builder().key_hints(KeyHints::Hidden).build()
    )]
    #[case::the_layout_mode_lands_in_the_layout_table(
        "layout_mode",
        "",
        AppearancePatch::builder().layout_mode(LayoutMode::Compact).build()
    )]
    #[case::one_field_keeps_every_comment(
        "one_field",
        COMMENTED_UI,
        AppearancePatch::builder().cover_brackets(CoverBrackets::Shown).build()
    )]
    #[case::animations_off(
        "animations_off",
        "",
        AppearancePatch::builder().animations(Animations::Off).build()
    )]
    #[case::animations_on(
        "animations_on",
        "",
        AppearancePatch::builder().animations(Animations::On).build()
    )]
    #[case::every_field_in_text_over_a_commented_file(
        "every_field_text",
        COMMENTED_UI,
        every_appearance_field_in_text()
    )]
    fn appearance_patched_writes(
        #[case] name: &str,
        #[case] text: &str,
        #[case] patch: AppearancePatch,
    ) {
        let out = appearance_patched(text, patch).unwrap();
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(out);
        });
    }

    #[test]
    fn appearance_patched_reports_not_a_table_instead_of_panicking() {
        let refused = appearance_patched(
            "card = \"x\"\n",
            AppearancePatch::builder()
                .format_chips(FormatChips::Shown)
                .build(),
        );
        assert!(
            matches!(refused, Err(ConfigError::NotATable { ref key }) if key == "card")
        );
    }

    fn text_at<'a>(parsed: &'a toml::Value, table: &str, key: &str) -> Option<&'a str> {
        parsed.get(table)?.get(key)?.as_str()
    }

    #[test]
    fn a_full_appearance_patch_still_parses_as_plain_toml() {
        let written =
            appearance_patched(COMMENTED_UI, every_appearance_field_in_text()).unwrap();
        let parsed: toml::Value = toml::from_str(&written).unwrap();
        assert_eq!(text_at(&parsed, "cover", "style"), Some("milkdrop"));
        assert_eq!(text_at(&parsed, "layout", "mode"), Some("compact"));
    }

    #[test]
    fn a_full_appearance_patch_reads_back_through_its_own_parser() {
        let written =
            appearance_patched(COMMENTED_UI, every_appearance_field_in_text()).unwrap();
        let parsed = parse_appearance(&written).unwrap();
        assert_eq!(
            parsed.options(),
            Appearance {
                cover_style: CoverStyle::Milkdrop,
                cover_brackets: CoverBrackets::Shown,
                format_chips: FormatChips::Shown,
                speed_chip: SpeedChipMode::Always,
                progress_remaining: ProgressStyle::Remaining,
                key_hints: KeyHints::Hidden,
                animations: Animations::Off,
                layout_mode: LayoutMode::Compact,
            }
        );
    }

    fn base_appearance_texts() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just(""), Just(COMMENTED_UI)]
    }

    type CoverCardProgressPatch = (
        Option<CoverStyle>,
        Option<CoverBrackets>,
        Option<FormatChips>,
        Option<SpeedChipMode>,
        Option<ProgressStyle>,
    );

    fn cover_card_progress_patch() -> impl Strategy<Value = CoverCardProgressPatch> {
        (
            proptest::option::of(prop_oneof![
                Just(CoverStyle::Vinyl),
                Just(CoverStyle::Plain),
                Just(CoverStyle::Milkdrop),
                Just(CoverStyle::Off),
            ]),
            proptest::option::of(prop_oneof![
                Just(CoverBrackets::Shown),
                Just(CoverBrackets::Hidden),
            ]),
            proptest::option::of(prop_oneof![
                Just(FormatChips::Shown),
                Just(FormatChips::Hidden),
            ]),
            proptest::option::of(prop_oneof![
                Just(SpeedChipMode::Always),
                Just(SpeedChipMode::Changed),
                Just(SpeedChipMode::Never),
            ]),
            proptest::option::of(prop_oneof![
                Just(ProgressStyle::Elapsed),
                Just(ProgressStyle::Remaining),
            ]),
        )
    }

    type WindowLayoutPatch = (Option<KeyHints>, Option<Animations>, Option<LayoutMode>);

    fn window_layout_patch() -> impl Strategy<Value = WindowLayoutPatch> {
        (
            proptest::option::of(prop_oneof![
                Just(KeyHints::Shown),
                Just(KeyHints::Hidden)
            ]),
            proptest::option::of(prop_oneof![
                Just(Animations::On),
                Just(Animations::Off)
            ]),
            proptest::option::of(prop_oneof![
                Just(LayoutMode::Auto),
                Just(LayoutMode::Full),
                Just(LayoutMode::Compact),
            ]),
        )
    }

    fn appearance_patch() -> impl Strategy<Value = AppearancePatch> {
        (cover_card_progress_patch(), window_layout_patch()).prop_map(
            |(
                (
                    cover_style,
                    cover_brackets,
                    format_chips,
                    speed_chip,
                    progress_remaining,
                ),
                (key_hints, animations, layout_mode),
            )| AppearancePatch {
                cover_style,
                cover_brackets,
                format_chips,
                speed_chip,
                progress_remaining,
                key_hints,
                animations,
                layout_mode,
            },
        )
    }

    proptest! {
        #[test]
        fn an_untouched_appearance_patch_leaves_the_document_unchanged(
            text in base_appearance_texts(),
        ) {
            let written = appearance_patched(text, AppearancePatch::builder().build()).unwrap();
            prop_assert_eq!(written, text);
        }

        #[test]
        fn an_appearance_patch_reads_back_exactly_what_it_wrote(
            text in base_appearance_texts(),
            patch in appearance_patch(),
        ) {
            let base = parse_appearance(text).unwrap().options();
            let written = appearance_patched(text, patch).unwrap();
            let parsed = parse_appearance(&written).unwrap().options();

            prop_assert_eq!(parsed.cover_style, patch.cover_style.unwrap_or(base.cover_style));
            prop_assert_eq!(
                parsed.cover_brackets,
                patch.cover_brackets.unwrap_or(base.cover_brackets)
            );
            prop_assert_eq!(
                parsed.format_chips,
                patch.format_chips.unwrap_or(base.format_chips)
            );
            prop_assert_eq!(parsed.speed_chip, patch.speed_chip.unwrap_or(base.speed_chip));
            prop_assert_eq!(
                parsed.progress_remaining,
                patch.progress_remaining.unwrap_or(base.progress_remaining)
            );
            prop_assert_eq!(parsed.key_hints, patch.key_hints.unwrap_or(base.key_hints));
            prop_assert_eq!(parsed.animations, patch.animations.unwrap_or(base.animations));
            prop_assert_eq!(parsed.layout_mode, patch.layout_mode.unwrap_or(base.layout_mode));
        }
    }
}
