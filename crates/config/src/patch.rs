use std::time::Duration;

use kernel::{
    cmd::ConfigPatch,
    domain::{
        appearance::{
            Animations,
            AppearancePatch,
            CoverBrackets,
            FormatChips,
            KeyHints,
            ProgressTime,
        },
        crossfade::Crossfade,
        device::OutputDevice,
        server::Account,
        settings::ReplayGain,
        theme::ThemeName,
        time::SECONDS_PER_MINUTE,
    },
};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

use crate::error::Error;

struct FieldEdit {
    table: &'static str,
    key: &'static str,
    item: Option<Item>,
}

fn ensure_table<'doc>(
    doc: &'doc mut DocumentMut,
    key: &'static str,
) -> Result<&'doc mut Table, Error> {
    doc.entry(key)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or(Error::NotATable(key))
}

fn write_edits<const N: usize>(
    doc: &mut DocumentMut,
    field_edits: [FieldEdit; N],
) -> Result<(), Error> {
    field_edits
        .into_iter()
        .filter_map(|edit| edit.item.map(|item| (edit.table, edit.key, item)))
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
        cover_mode,
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
            FieldEdit {
                table: "cover",
                key: "mode",
                item: cover_mode.map(|s| value(s.to_string())),
            },
            FieldEdit {
                table: "cover",
                key: "brackets",
                item: cover_brackets.map(|b| value(matches!(b, CoverBrackets::Shown))),
            },
            FieldEdit {
                table: "card",
                key: "format_chips",
                item: format_chips.map(|c| value(matches!(c, FormatChips::Shown))),
            },
            FieldEdit {
                table: "card",
                key: "speed_chip",
                item: speed_chip.map(|s| value(s.to_string())),
            },
            FieldEdit {
                table: "progress",
                key: "remaining",
                item: progress_time
                    .map(|t| value(matches!(t, ProgressTime::Remaining))),
            },
            FieldEdit {
                table: "window",
                key: "key_hints",
                item: key_hints.map(|h| value(matches!(h, KeyHints::Shown))),
            },
            FieldEdit {
                table: "window",
                key: "animations",
                item: animations.map(|a| value(matches!(a, Animations::On))),
            },
            FieldEdit {
                table: "layout",
                key: "mode",
                item: layout_mode.map(|m| value(m.to_string())),
            },
        ],
    )
}

pub fn patched_appearance_text(
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

fn minutes(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs() / SECONDS_PER_MINUTE).unwrap_or(i64::MAX)
}

fn write_device(
    doc: &mut DocumentMut,
    device: Option<OutputDevice>,
) -> Result<(), Error> {
    match device {
        None => {}
        Some(OutputDevice::Named(name)) => {
            ensure_table(doc, "audio")?["device"] = value(name.as_str());
        }
        Some(OutputDevice::SystemDefault) => {
            ensure_table(doc, "audio")?.remove("device");
        }
    }
    Ok(())
}

fn write_servers(doc: &mut DocumentMut, accounts: Option<Vec<Account>>) {
    let Some(accounts) = accounts else {
        return;
    };
    if accounts.is_empty() {
        doc.remove("server");
        return;
    }
    doc["server"] = Item::ArrayOfTables(
        accounts
            .iter()
            .map(|account| {
                Table::from_iter([
                    ("name", account.server_name.as_str()),
                    ("url", account.endpoint.as_str()),
                    ("user", account.user_name.as_str()),
                ])
            })
            .collect::<ArrayOfTables>(),
    );
}

fn patch_config(doc: &mut DocumentMut, patch: ConfigPatch) -> Result<(), Error> {
    let ConfigPatch {
        crossfade,
        device,
        replay_gain,
        theme_name,
        volume,
        sleep_presets,
        music_dir,
        accounts,
    } = patch;
    write_servers(doc, accounts);
    write_edits(
        doc,
        [
            FieldEdit {
                table: "audio",
                key: "crossfade",
                item: crossfade.map(|c| value(format_crossfade(c))),
            },
            FieldEdit {
                table: "audio",
                key: "replay_gain",
                item: replay_gain.map(|r| value(matches!(r, ReplayGain::On))),
            },
        ],
    )?;
    write_device(doc, device)?;
    write_edits(
        doc,
        [
            FieldEdit {
                table: "audio",
                key: "sleep_presets",
                item: sleep_presets.map(|presets| {
                    value(Array::from_iter(
                        presets.as_slice().iter().copied().map(minutes),
                    ))
                }),
            },
            FieldEdit {
                table: "",
                key: "theme",
                item: theme_name.map(|t| value(ThemeName::as_str(&t))),
            },
            FieldEdit {
                table: "",
                key: "volume",
                item: volume.map(|v| value(i64::from(v.get()))),
            },
            FieldEdit {
                table: "",
                key: "music_dir",
                item: music_dir.map(|dir| value(dir.to_string_lossy().into_owned())),
            },
        ],
    )
}

pub fn patched_config_text(text: &str, patch: ConfigPatch) -> Result<String, Error> {
    let mut doc: DocumentMut = text.parse()?;
    patch_config(&mut doc, patch)?;
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::{
        cmd::ConfigPatch,
        domain::{
            appearance::{
                Animations,
                AppearancePatch,
                CoverBrackets,
                CoverMode,
                FormatChips,
                KeyHints,
                LayoutMode,
                ProgressTime,
                SpeedChip,
            },
            appearance_rows::{
                ANIMATIONS,
                COVER_BRACKETS,
                COVER_MODES,
                FORMAT_CHIPS,
                KEY_HINTS,
                LAYOUT_MODES,
                PROGRESS_TIMES,
                SPEED_CHIPS,
            },
            bounded::Bounded,
            crossfade::Crossfade,
            device::{DeviceName, OutputDevice},
            percent::Percent,
            server::{Account, Endpoint, ServerName, UserName},
            setting_row::{AppearanceField, OptionCount},
            settings::ReplayGain,
            sleep_presets::SleepPresets,
            theme::{ThemeChoice, ThemeName},
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
        appearance_file::parse_appearance,
        config_file::{parse_config, parse_config_settings},
        error::Error,
        patch::{
            format_crossfade,
            minutes,
            patched_appearance_text,
            patched_config_text,
        },
    };

    const COMMENTED_UI: &str = include_str!("../tests/fixtures/sifr-ui_commented.toml");

    fn every_appearance_field_in_text() -> AppearancePatch {
        AppearancePatch {
            cover_mode: Some(CoverMode::Milkdrop),
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
        AppearancePatch::default()
    )]
    #[case::minimal_sections_on_an_empty_document(
        "minimal_sections",
        "",
        AppearancePatch {
            cover_mode: Some(CoverMode::Off),
            format_chips: Some(FormatChips::Shown),
            ..AppearancePatch::default()
        }
    )]
    #[case::the_key_hints_lands_in_the_window_table(
        "key_hints",
        "",
        AppearancePatch {
            key_hints: Some(KeyHints::Hidden),
            ..AppearancePatch::default()
        }
    )]
    #[case::the_layout_mode_lands_in_the_layout_table(
        "layout_mode",
        "",
        AppearancePatch {
            layout_mode: Some(LayoutMode::Compact),
            ..AppearancePatch::default()
        }
    )]
    #[case::one_field_keeps_every_comment(
        "one_field",
        COMMENTED_UI,
        AppearancePatch {
            cover_brackets: Some(CoverBrackets::Shown),
            ..AppearancePatch::default()
        }
    )]
    #[case::animations_off(
        "animations_off",
        "",
        AppearancePatch {
            animations: Some(Animations::Off),
            ..AppearancePatch::default()
        }
    )]
    #[case::animations_on(
        "animations_on",
        "",
        AppearancePatch {
            animations: Some(Animations::On),
            ..AppearancePatch::default()
        }
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
        let out = patched_appearance_text(text, patch).unwrap();
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(out);
        });
    }

    #[rstest]
    #[case::cover_mode("cover_mode", AppearanceField::CoverMode, 3)]
    #[case::key_hints("key_hints", AppearanceField::KeyHints, 1)]
    #[case::layout_mode("layout_mode", AppearanceField::LayoutMode, 1)]
    fn an_effect_lands_in_the_file_it_belongs_to(
        #[case] name: &str,
        #[case] field: AppearanceField,
        #[case] option_index: usize,
    ) {
        let option = OptionCount::new(option_index + 1)
            .and_then(|count| count.index(option_index))
            .unwrap();
        let patch =
            kernel::domain::appearance_rows::appearance_patch(field, option).unwrap();
        let written = patched_appearance_text("", patch).unwrap();

        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(written);
        });
    }

    #[test]
    fn an_appearance_patch_reports_a_non_table_document_instead_of_panicking() {
        let refused = patched_appearance_text(
            "card = \"x\"\n",
            AppearancePatch {
                format_chips: Some(FormatChips::Shown),
                ..AppearancePatch::default()
            },
        );
        assert!(matches!(refused, Err(Error::NotATable("card"))));
    }

    fn base_appearance_texts() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just(""), Just(COMMENTED_UI)]
    }

    fn appearance_patch() -> impl Strategy<Value = AppearancePatch> {
        (
            option_of(select(COVER_MODES.to_vec())),
            option_of(select(COVER_BRACKETS.to_vec())),
            option_of(select(FORMAT_CHIPS.to_vec())),
            option_of(select(SPEED_CHIPS.to_vec())),
            option_of(select(PROGRESS_TIMES.to_vec())),
            option_of(select(KEY_HINTS.to_vec())),
            option_of(select(ANIMATIONS.to_vec())),
            option_of(select(LAYOUT_MODES.to_vec())),
        )
            .prop_map(
                |(
                    cover_mode,
                    cover_brackets,
                    format_chips,
                    speed_chip,
                    progress_time,
                    key_hints,
                    animations,
                    layout_mode,
                )| AppearancePatch {
                    cover_mode,
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
            let written = patched_appearance_text(text, AppearancePatch::default()).unwrap();
            prop_assert_eq!(written, text);
        }

        #[test]
        fn an_appearance_patch_reads_back_exactly_what_it_wrote(
            text in base_appearance_texts(),
            patch in appearance_patch(),
        ) {
            let base = parse_appearance(text).unwrap().to_appearance_settings();
            let written = patched_appearance_text(text, patch).unwrap();
            let parsed = parse_appearance(&written).unwrap().to_appearance_settings();

            prop_assert_eq!(parsed.cover_mode, patch.cover_mode.unwrap_or(base.cover_mode));
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
        "[audio]\ndevice = \"Speakers\"\nreplay_gain = false\n";

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
            theme_name: Some(ThemeName::from_static("oreo")),
            volume: Some(Percent::clamped(80)),
            sleep_presets: Some(SleepPresets::from_minutes(&[10, 20]).unwrap()),
            music_dir: Some("/new/music".into()),
            accounts: None,
        }
    }

    fn accounts() -> Vec<Account> {
        [
            ("home", "https://music.example", "ann"),
            ("work", "http://10.0.0.2:4533", "bob"),
        ]
        .into_iter()
        .map(|(name, link, user)| Account {
            server_name: ServerName::new(name),
            endpoint: Endpoint::parse(link).unwrap(),
            user_name: UserName::new(user).unwrap(),
        })
        .collect()
    }

    #[test]
    fn a_servers_patch_reads_back_its_accounts_and_keeps_every_comment() {
        let written = patched_config_text(
            COMMENTED_CONFIG,
            ConfigPatch {
                accounts: Some(accounts()),
                ..ConfigPatch::default()
            },
        )
        .unwrap();
        let emptied = patched_config_text(
            &written,
            ConfigPatch {
                accounts: Some(Vec::new()),
                ..ConfigPatch::default()
            },
        )
        .unwrap();

        assert_eq!(
            parse_config_settings(&written)
                .map(|settings| settings.accounts)
                .ok(),
            Some(accounts())
        );
        assert!(
            COMMENTED_CONFIG
                .lines()
                .filter(|line| line.contains('#'))
                .all(|line| written.contains(line)),
            "was {written}"
        );
        assert_eq!(
            parse_config_settings(&emptied)
                .map(|settings| settings.accounts)
                .ok(),
            Some(Vec::new())
        );
        assert!(!emptied.contains("[[server]]"), "was {emptied}");
    }

    #[rstest]
    #[case::an_empty_patch_changes_nothing(
        "empty_patch",
        COMMENTED_CONFIG,
        ConfigPatch::default()
    )]
    #[case::minimal_sections_on_an_empty_document(
        "minimal_sections",
        "",
        ConfigPatch {
            crossfade: Some(crossfade_seconds(3)),
            theme_name: Some(ThemeName::from_static("dark")),
            ..ConfigPatch::default()
        }
    )]
    #[case::one_field_keeps_every_comment(
        "one_field",
        COMMENTED_CONFIG,
        ConfigPatch {
            crossfade: Some(crossfade_seconds(3)),
            ..ConfigPatch::default()
        }
    )]
    #[case::every_field_kind("every_field", "", every_config_field())]
    #[case::the_system_default_device_removes_the_key(
        "device_removed",
        CONFIG_WITH_DEVICE,
        ConfigPatch {
            device: Some(OutputDevice::SystemDefault),
            ..ConfigPatch::default()
        }
    )]
    fn a_patch_writes_only_the_fields_it_sets(
        #[case] name: &str,
        #[case] text: &str,
        #[case] patch: ConfigPatch,
    ) {
        let out = patched_config_text(text, patch).unwrap();
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
    fn minutes_drops_sub_minute_remainder(
        #[case] secs: u64,
        #[case] whole_minutes: i64,
    ) {
        assert_eq!(minutes(Duration::from_secs(secs)), whole_minutes);
    }

    #[test]
    fn a_patch_reports_a_non_table_document_instead_of_panicking() {
        let refused = patched_config_text(
            "audio = 1\n",
            ConfigPatch {
                crossfade: Some(crossfade_seconds(3)),
                ..ConfigPatch::default()
            },
        );
        assert!(matches!(refused, Err(Error::NotATable("audio"))));
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
            proptest::option::of(prop_oneof![
                Just(Vec::new()),
                Just(accounts()),
                Just(accounts().split_off(1)),
            ]),
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
                    accounts,
                )| {
                    ConfigPatch {
                        crossfade,
                        device,
                        replay_gain,
                        theme_name: theme,
                        volume,
                        sleep_presets,
                        music_dir,
                        accounts,
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
                let written = patched_config_text(text, patch.clone()).unwrap();
                let parsed = parse_config(&written).unwrap();
                let base_accounts = parse_config_settings(text).unwrap().accounts;
                let parsed_accounts = parse_config_settings(&written).unwrap().accounts;

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
                    parsed.theme_choice,
                    patch.theme_name.map_or(base.theme_choice, ThemeChoice::Named)
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
                prop_assert_eq!(
                    parsed_accounts,
                    patch.accounts.unwrap_or(base_accounts)
                );
            }
        }
}
