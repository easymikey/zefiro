use std::time::Duration;

use kernel::{
    ConfigPatch,
    DevicePatch,
    domain::{Crossfade, Replaygain, ThemeName},
};
use toml_edit::{Array, DocumentMut, value};

use crate::{
    document::{TomlEdit, ensure_table, write_edits},
    error::Error,
};

fn format_crossfade(crossfade: Crossfade) -> String {
    let duration = crossfade.value();
    if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn to_minutes(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs() / 60).unwrap_or(i64::MAX)
}

fn crossfade_replaygain_fields(
    crossfade: Option<Crossfade>,
    replaygain: Option<Replaygain>,
) -> [TomlEdit; 2] {
    [
        (
            "audio",
            "crossfade",
            crossfade.map(|crossfade| value(format_crossfade(crossfade))),
        ),
        (
            "audio",
            "replaygain",
            replaygain.map(|replaygain| value(matches!(replaygain, Replaygain::On))),
        ),
    ]
}

fn sleep_presets_field(sleep_presets: Option<Vec<Duration>>) -> [TomlEdit; 1] {
    [(
        "audio",
        "sleep_presets",
        sleep_presets.map(|presets| {
            value(presets.iter().copied().map(to_minutes).collect::<Array>())
        }),
    )]
}

fn patch_config(doc: &mut DocumentMut, patch: ConfigPatch) -> Result<(), Error> {
    let ConfigPatch {
        crossfade,
        device,
        replaygain,
        theme,
        volume,
        sleep_presets,
        music_dir,
    } = patch;
    write_edits(doc, crossfade_replaygain_fields(crossfade, replaygain))?;
    match device {
        DevicePatch::Keep => {}
        DevicePatch::Named(name) => {
            ensure_table(doc, "audio")?["device"] = value(name.as_str());
        }
        DevicePatch::SystemDefault => {
            ensure_table(doc, "audio")?.remove("device");
        }
    }
    write_edits(doc, sleep_presets_field(sleep_presets))?;
    if let Some(theme) = theme {
        doc["theme"] = value(ThemeName::as_str(&theme));
    }
    if let Some(volume) = volume {
        doc["volume"] = value(i64::from(volume.value()));
    }
    if let Some(music_dir) = music_dir {
        doc["music_dir"] = value(music_dir);
    }
    Ok(())
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
        DevicePatch,
        domain::{
            Crossfade,
            DeviceName,
            OutputDevice,
            Percent,
            Replaygain,
            ThemeChoice,
            ThemeName,
        },
    };
    use proptest::prelude::*;
    use rstest::rstest;

    use crate::{
        appearance_file::parse_appearance,
        config_document::{format_crossfade, patch_config_text, to_minutes},
        config_file::parse_config,
        error::Error,
    };

    const COMMENTED_CONFIG: &str =
        include_str!("../tests/fixtures/config_commented.toml");

    const COMMENTED_UI: &str = include_str!("../tests/fixtures/sifr-ui_commented.toml");

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
            replaygain: Some(Replaygain::On),
            device: DevicePatch::Named(
                DeviceName::new("Speakers".to_string()).unwrap(),
            ),
            theme: Some(ThemeName::from_static("oreo")),
            volume: Some(Percent::clamped(80)),
            sleep_presets: Some(vec![
                Duration::from_secs(10 * 60),
                Duration::from_secs(20 * 60),
            ]),
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
        ConfigPatch::builder().device(DevicePatch::SystemDefault).build()
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

    #[test]
    fn every_fixture_parses_with_its_own_parser() {
        assert!(parse_config(COMMENTED_CONFIG).is_ok());
        assert!(parse_appearance(COMMENTED_UI).is_ok());
    }

    #[test]
    fn a_full_config_patch_reads_back_through_its_own_parser() {
        let written =
            patch_config_text(COMMENTED_CONFIG, every_config_field()).unwrap();
        let parsed = parse_config(&written).unwrap();
        assert_eq!(parsed.audio.crossfade, crossfade_milliseconds(250));
        assert_eq!(parsed.audio.replaygain, Replaygain::On);
        assert_eq!(
            parsed.audio.device,
            OutputDevice::Named(DeviceName::new("Speakers".to_string()).unwrap())
        );
        assert_eq!(
            parsed.theme,
            ThemeChoice::Named(ThemeName::from_static("oreo"))
        );
        assert_eq!(parsed.volume, Percent::clamped(80));
        assert_eq!(
            parsed.audio.sleep_presets.as_slice(),
            [Duration::from_secs(10 * 60), Duration::from_secs(20 * 60)]
        );
        assert_eq!(parsed.music_dir, Some(PathBuf::from("/new/music")));
    }

    fn base_config_texts() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just(""), Just(COMMENTED_CONFIG)]
    }

    fn device_patch() -> impl Strategy<Value = DevicePatch> {
        prop_oneof![
            Just(DevicePatch::Keep),
            Just(DevicePatch::SystemDefault),
            prop_oneof![Just("Speakers"), Just("Headphones")].prop_map(|name| {
                DevicePatch::Named(DeviceName::new(name.to_string()).unwrap())
            }),
        ]
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
                Just(Replaygain::On),
                Just(Replaygain::Off)
            ]),
            proptest::option::of(
                prop_oneof![Just("dark"), Just("oreo"), Just("noir")]
                    .prop_map(|name| ThemeName::new(name.to_string()).unwrap()),
            ),
            proptest::option::of((0u8..=100).prop_map(Percent::clamped)),
            proptest::option::of(
                proptest::collection::btree_set(1u64..=120, 0..4).prop_map(|minutes| {
                    minutes
                        .into_iter()
                        .map(|value| Duration::from_secs(value * 60))
                        .collect::<Vec<_>>()
                }),
            ),
            proptest::option::of(
                prop_oneof![Just("/music"), Just("/new/music")]
                    .prop_map(str::to_string),
            ),
        )
            .prop_map(
                |(
                    crossfade,
                    device,
                    replaygain,
                    theme,
                    volume,
                    sleep_presets,
                    music_dir,
                )| {
                    ConfigPatch {
                        crossfade,
                        device,
                        replaygain,
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
        fn an_untouched_config_patch_leaves_the_document_unchanged(text in base_config_texts()) {
            let written = patch_config_text(text, ConfigPatch::builder().build()).unwrap();
            prop_assert_eq!(written, text);
        }

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
                parsed.audio.replaygain,
                patch.replaygain.unwrap_or(base.audio.replaygain)
            );
            prop_assert_eq!(
                parsed.audio.device,
                match patch.device {
                    DevicePatch::Keep => base.audio.device,
                    DevicePatch::SystemDefault => OutputDevice::SystemDefault,
                    DevicePatch::Named(name) => OutputDevice::Named(name),
                }
            );
            prop_assert_eq!(
                parsed.theme,
                patch.theme.map_or(base.theme, ThemeChoice::Named)
            );
            prop_assert_eq!(parsed.volume, patch.volume.unwrap_or(base.volume));
            prop_assert_eq!(
                parsed.audio.sleep_presets.as_slice(),
                patch
                    .sleep_presets
                    .as_deref()
                    .unwrap_or(base.audio.sleep_presets.as_slice())
            );
            prop_assert_eq!(
                parsed.music_dir,
                patch.music_dir.map(PathBuf::from).or(base.music_dir)
            );
        }
    }
}
