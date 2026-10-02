use std::time::Duration;

use kernel::{
    ConfigPatch,
    domain::{Crossfade, OutputDevice, ReplayGain, ThemeName},
};
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::{
    appearance::{
        Animations,
        AppearancePatch,
        CoverBrackets,
        FormatChips,
        KeyHints,
        ProgressTime,
    },
    error::Error,
};

type TomlEdit = (&'static str, &'static str, Option<Item>);

fn ensure_table<'doc>(
    doc: &'doc mut DocumentMut,
    key: &str,
) -> Result<&'doc mut Table, Error> {
    doc.entry(key)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| Error::NotATable {
            key: key.to_string(),
        })
}

fn write_edits<const N: usize>(
    doc: &mut DocumentMut,
    fields: [TomlEdit; N],
) -> Result<(), Error> {
    fields
        .into_iter()
        .filter_map(|(table, key, item)| item.map(|item| (table, key, item)))
        .try_for_each(|(table, key, item)| {
            let target = if table.is_empty() {
                doc.as_table_mut()
            } else {
                ensure_table(doc, table)?
            };
            target[key] = item;
            Ok(())
        })
}

fn patch_appearance(
    doc: &mut DocumentMut,
    patch: AppearancePatch,
) -> Result<(), Error> {
    let AppearancePatch {
        cover_style,
        cover_brackets,
        format_chips,
        speed_chip,
        progress_time,
        key_hints,
        animations,
        layout_mode,
    } = patch;
    write_edits(
        doc,
        [
            ("cover", "style", cover_style.map(|s| value(s.to_string()))),
            (
                "cover",
                "brackets",
                cover_brackets.map(|b| value(matches!(b, CoverBrackets::Shown))),
            ),
            (
                "card",
                "format_chips",
                format_chips.map(|c| value(matches!(c, FormatChips::Shown))),
            ),
            (
                "card",
                "speed_chip",
                speed_chip.map(|s| value(s.to_string())),
            ),
            (
                "progress",
                "remaining",
                progress_time.map(|t| value(matches!(t, ProgressTime::Remaining))),
            ),
            (
                "window",
                "key_hints",
                key_hints.map(|h| value(matches!(h, KeyHints::Shown))),
            ),
            (
                "window",
                "animations",
                animations.map(|a| value(matches!(a, Animations::On))),
            ),
            ("layout", "mode", layout_mode.map(|m| value(m.to_string()))),
        ],
    )
}

pub fn patch_appearance_text(
    text: &str,
    patch: AppearancePatch,
) -> Result<String, Error> {
    let mut doc: DocumentMut = text.parse()?;
    patch_appearance(&mut doc, patch)?;
    Ok(doc.to_string())
}

fn format_crossfade(crossfade: Crossfade) -> String {
    let duration = crossfade.get();
    if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn to_minutes(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs() / 60).unwrap_or(i64::MAX)
}

fn patch_config(doc: &mut DocumentMut, patch: ConfigPatch) -> Result<(), Error> {
    let ConfigPatch {
        crossfade,
        device,
        replay_gain,
        theme,
        volume,
        sleep_presets,
        music_dir,
    } = patch;
    write_edits(
        doc,
        [
            (
                "audio",
                "crossfade",
                crossfade.map(|c| value(format_crossfade(c))),
            ),
            (
                "audio",
                "replaygain",
                replay_gain.map(|r| value(matches!(r, ReplayGain::On))),
            ),
        ],
    )?;
    match device {
        None => {}
        Some(OutputDevice::Named(name)) => {
            ensure_table(doc, "audio")?["device"] = value(name.as_str());
        }
        Some(OutputDevice::SystemDefault) => {
            ensure_table(doc, "audio")?.remove("device");
        }
    }
    write_edits(
        doc,
        [
            (
                "audio",
                "sleep_presets",
                sleep_presets.map(|presets| {
                    value(
                        presets
                            .as_slice()
                            .iter()
                            .copied()
                            .map(to_minutes)
                            .collect::<Array>(),
                    )
                }),
            ),
            ("", "theme", theme.map(|t| value(ThemeName::as_str(&t)))),
            ("", "volume", volume.map(|v| value(i64::from(v.get())))),
            (
                "",
                "music_dir",
                music_dir.map(|dir| value(dir.to_string_lossy().into_owned())),
            ),
        ],
    )
}

pub fn patch_config_text(text: &str, patch: ConfigPatch) -> Result<String, Error> {
    let mut doc: DocumentMut = text.parse()?;
    patch_config(&mut doc, patch)?;
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::{
        Bounded,
        ConfigPatch,
        domain::{
            Crossfade,
            DeviceName,
            OptionCount,
            OutputDevice,
            Percent,
            ReplayGain,
            SleepPresets,
            ThemeChoice,
            ThemeName,
            appearance_rows::{
                ANIMATIONS,
                AppearanceField,
                COVER_BRACKETS,
                COVER_STYLES,
                FORMAT_CHIPS,
                KEY_HINTS,
                LAYOUT_MODES,
                PROGRESS_STYLES,
                SPEED_CHIPS,
            },
        },
    };
    use proptest::{
        option::of as option_of,
        prelude::{Just, Strategy},
        prop_assert_eq,
        prop_oneof,
        proptest,
        sample::select,
    };
    use rstest::rstest;

    use crate::{
        appearance::{
            Animations,
            AppearancePatch,
            CoverBrackets,
            CoverStyle,
            FormatChips,
            KeyHints,
            LayoutMode,
            ProgressTime,
            SpeedChip,
        },
        appearance_file::parse_appearance,
        config_file::parse_config,
        error::Error,
        patch::{
            format_crossfade,
            patch_appearance_text,
            patch_config_text,
            to_minutes,
        },
    };

    const COMMENTED_UI: &str = include_str!("../tests/fixtures/sifr-ui_commented.toml");

    fn every_appearance_field_in_text() -> AppearancePatch {
        AppearancePatch {
            cover_style: Some(CoverStyle::Milkdrop),
            cover_brackets: Some(CoverBrackets::Shown),
            format_chips: Some(FormatChips::Shown),
            speed_chip: Some(SpeedChip::Always),
            progress_time: Some(ProgressTime::Remaining),
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
    fn an_appearance_patch_writes_only_the_fields_it_sets(
        #[case] name: &str,
        #[case] text: &str,
        #[case] patch: AppearancePatch,
    ) {
        let out = patch_appearance_text(text, patch).unwrap();
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(out);
        });
    }

    #[rstest]
    #[case::cover_style("cover_style", AppearanceField::CoverStyle, 3)]
    #[case::key_hints("key_hints", AppearanceField::KeyHints, 1)]
    #[case::layout_mode("layout_mode", AppearanceField::LayoutMode, 2)]
    fn an_effect_lands_in_the_file_it_belongs_to(
        #[case] name: &str,
        #[case] id: AppearanceField,
        #[case] position: usize,
    ) {
        let option = OptionCount::new(position + 1)
            .and_then(|count| count.index(position))
            .unwrap();
        let patch =
            kernel::domain::appearance_rows::appearance_patch(id, option).unwrap();
        let written = patch_appearance_text("", patch).unwrap();

        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(written);
        });
    }

    #[test]
    fn an_appearance_patch_reports_a_non_table_document_instead_of_panicking() {
        let refused = patch_appearance_text(
            "card = \"x\"\n",
            AppearancePatch::builder()
                .format_chips(FormatChips::Shown)
                .build(),
        );
        assert!(matches!(refused, Err(Error::NotATable { ref key }) if key == "card"));
    }

    fn base_appearance_texts() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just(""), Just(COMMENTED_UI)]
    }

    fn appearance_patch() -> impl Strategy<Value = AppearancePatch> {
        (
            option_of(select(COVER_STYLES.to_vec())),
            option_of(select(COVER_BRACKETS.to_vec())),
            option_of(select(FORMAT_CHIPS.to_vec())),
            option_of(select(SPEED_CHIPS.to_vec())),
            option_of(select(PROGRESS_STYLES.to_vec())),
            option_of(select(KEY_HINTS.to_vec())),
            option_of(select(ANIMATIONS.to_vec())),
            option_of(select(LAYOUT_MODES.to_vec())),
        )
            .prop_map(
                |(
                    cover_style,
                    cover_brackets,
                    format_chips,
                    speed_chip,
                    progress_time,
                    key_hints,
                    animations,
                    layout_mode,
                )| AppearancePatch {
                    cover_style,
                    cover_brackets,
                    format_chips,
                    speed_chip,
                    progress_time,
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
            let written = patch_appearance_text(text, AppearancePatch::builder().build()).unwrap();
            prop_assert_eq!(written, text);
        }

        #[test]
        fn an_appearance_patch_reads_back_exactly_what_it_wrote(
            text in base_appearance_texts(),
            patch in appearance_patch(),
        ) {
            let base = parse_appearance(text).unwrap().appearance();
            let written = patch_appearance_text(text, patch).unwrap();
            let parsed = parse_appearance(&written).unwrap().appearance();

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
                parsed.progress_time,
                patch.progress_time.unwrap_or(base.progress_time)
            );
            prop_assert_eq!(parsed.key_hints, patch.key_hints.unwrap_or(base.key_hints));
            prop_assert_eq!(parsed.animations, patch.animations.unwrap_or(base.animations));
            prop_assert_eq!(parsed.layout_mode, patch.layout_mode.unwrap_or(base.layout_mode));
        }
    }

    const COMMENTED_CONFIG: &str =
        include_str!("../tests/fixtures/config_commented.toml");

    const CONFIG_WITH_DEVICE: &str =
        "[audio]\ndevice = \"Speakers\"\nreplaygain = false\n";

    fn crossfade_seconds(secs: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(secs))
    }

    fn crossfade_milliseconds(millis: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_millis(millis))
    }

    fn every_config_field() -> ConfigPatch {
        ConfigPatch {
            crossfade: Some(crossfade_milliseconds(250)),
            replay_gain: Some(ReplayGain::On),
            device: Some(OutputDevice::Named(
                DeviceName::new("Speakers".to_string()).unwrap(),
            )),
            theme: Some(ThemeName::from_static("oreo")),
            volume: Some(Percent::clamped(80)),
            sleep_presets: Some(SleepPresets::from_minutes(&[10, 20]).unwrap()),
            music_dir: Some("/new/music".into()),
        }
    }

    #[rstest]
    #[case::an_empty_patch_changes_nothing(
        "empty_patch",
        COMMENTED_CONFIG,
        ConfigPatch::builder().build()
    )]
    #[case::minimal_sections_on_an_empty_document(
        "minimal_sections",
        "",
        ConfigPatch::builder()
            .crossfade(crossfade_seconds(3))
            .theme(ThemeName::from_static("dark"))
            .build()
    )]
    #[case::one_field_keeps_every_comment(
        "one_field",
        COMMENTED_CONFIG,
        ConfigPatch::builder().crossfade(crossfade_seconds(3)).build()
    )]
    #[case::every_field_kind("every_field", "", every_config_field())]
    #[case::the_system_default_device_removes_the_key(
        "device_removed",
        CONFIG_WITH_DEVICE,
        ConfigPatch::builder().device(OutputDevice::SystemDefault).build()
    )]
    fn a_patch_writes_only_the_fields_it_sets(
        #[case] name: &str,
        #[case] text: &str,
        #[case] patch: ConfigPatch,
    ) {
        let out = patch_config_text(text, patch).unwrap();
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(out);
        });
    }

    #[rstest]
    #[case(Crossfade::default(), "0s")]
    #[case(crossfade_seconds(3), "3s")]
    #[case(crossfade_milliseconds(250), "250ms")]
    fn format_crossfade_round_trips_seconds_and_milliseconds(
        #[case] crossfade: Crossfade,
        #[case] written: &str,
    ) {
        assert_eq!(format_crossfade(crossfade), written);
    }

    #[rstest]
    #[case(90, 1)]
    #[case(15 * 60, 15)]
    fn to_minutes_drops_sub_minute_remainder(#[case] secs: u64, #[case] minutes: i64) {
        assert_eq!(to_minutes(Duration::from_secs(secs)), minutes);
    }

    #[test]
    fn a_patch_reports_a_non_table_document_instead_of_panicking() {
        let refused = patch_config_text(
            "audio = 1\n",
            ConfigPatch::builder()
                .crossfade(crossfade_seconds(3))
                .build(),
        );
        assert!(matches!(refused, Err(Error::NotATable { ref key }) if key == "audio"));
    }

    fn base_config_texts() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just(""), Just(COMMENTED_CONFIG)]
    }

    fn device_patch() -> impl Strategy<Value = Option<OutputDevice>> {
        proptest::option::of(prop_oneof![
            Just(OutputDevice::SystemDefault),
            prop_oneof![Just("Speakers"), Just("Headphones")].prop_map(|name| {
                OutputDevice::Named(DeviceName::new(name.to_string()).unwrap())
            }),
        ])
    }

    fn config_patch() -> impl Strategy<Value = ConfigPatch> {
        (
            proptest::option::of(
                (0u64..=10_000).prop_map(|millis| {
                    Crossfade::clamped(Duration::from_millis(millis))
                }),
            ),
            device_patch(),
            proptest::option::of(prop_oneof![
                Just(ReplayGain::On),
                Just(ReplayGain::Off)
            ]),
            proptest::option::of(
                prop_oneof![Just("dark"), Just("oreo"), Just("noir")]
                    .prop_map(|name| ThemeName::new(name.to_string()).unwrap()),
            ),
            proptest::option::of((0u8..=100).prop_map(Percent::clamped)),
            proptest::option::of(
                proptest::collection::btree_set(1u64..=120, 0..4).prop_map(|minutes| {
                    SleepPresets::from_minutes(&minutes.into_iter().collect::<Vec<_>>())
                        .unwrap()
                }),
            ),
            proptest::option::of(
                prop_oneof![Just("/music"), Just("/new/music")].prop_map(PathBuf::from),
            ),
        )
            .prop_map(
                |(
                    crossfade,
                    device,
                    replay_gain,
                    theme,
                    volume,
                    sleep_presets,
                    music_dir,
                )| {
                    ConfigPatch {
                        crossfade,
                        device,
                        replay_gain,
                        theme,
                        volume,
                        sleep_presets,
                        music_dir,
                    }
                },
            )
    }

    proptest! {
            #[test]
            fn a_config_patch_reads_back_exactly_what_it_wrote(
                text in base_config_texts(),
                patch in config_patch(),
            ) {
                let base = parse_config(text).unwrap();
                let written = patch_config_text(text, patch.clone()).unwrap();
                let parsed = parse_config(&written).unwrap();

                prop_assert_eq!(
                    parsed.audio.crossfade,
                    patch.crossfade.unwrap_or(base.audio.crossfade)
                );
                prop_assert_eq!(
                    parsed.audio.replay_gain,
                    patch.replay_gain.unwrap_or(base.audio.replay_gain)
                );
                prop_assert_eq!(
                    parsed.audio.device,
    patch.device.unwrap_or(base.audio.device)
                );
                prop_assert_eq!(
                    parsed.theme,
                    patch.theme.map_or(base.theme, ThemeChoice::Named)
                );
                prop_assert_eq!(parsed.volume, patch.volume.unwrap_or(base.volume));
                prop_assert_eq!(
                    parsed.audio.sleep_presets,
                    patch.sleep_presets.unwrap_or(base.audio.sleep_presets)
                );
                prop_assert_eq!(
                    parsed.music_dir,
                    patch.music_dir.or(base.music_dir)
                );
            }
        }
}
