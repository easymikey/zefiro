use config::{
    config_file::{TomlSettings, parse_config, parse_config_settings},
    patch::patched_config_text,
};
use kernel::{
    cmd::ConfigPatch,
    domain::{
        bounded::Bounded,
        device::{DeviceName, OutputDevice},
        percent::Percent,
        settings::AudioSettings,
        theme::{ThemeChoice, ThemeName},
    },
};
use rstest::rstest;

const COMMENTED_CONFIG: &str = include_str!("../../config.toml");

fn uncommented(text: &str) -> String {
    text.lines()
        .map(|line| line.strip_prefix("# ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn template_keys() -> Vec<(String, String)> {
    COMMENTED_CONFIG
        .lines()
        .map(|line| line.trim_start_matches(['#', ' ']))
        .scan(String::new(), |table, line| {
            let key = if line.starts_with('[') {
                let path = line.trim_matches(['[', ']']);
                *table = path.to_owned();
                let (parent, name) = path.rsplit_once('.').unwrap_or(("", path));
                Some((parent.to_owned(), name.to_owned()))
            } else {
                line.split_once(" = ")
                    .filter(|(name, _)| {
                        name.chars()
                            .all(|letter| letter.is_ascii_lowercase() || letter == '_')
                    })
                    .map(|(name, _)| (table.clone(), name.to_owned()))
            };
            Some(key)
        })
        .flatten()
        .collect()
}

#[test]
fn the_template_parses_to_the_defaults_commented_and_uncommented() {
    assert_eq!(
        parse_config(COMMENTED_CONFIG).unwrap(),
        TomlSettings::default()
    );
    assert_eq!(
        parse_config(&uncommented(COMMENTED_CONFIG)).unwrap(),
        TomlSettings::default()
    );
}

#[rstest]
#[case::top_level("", "zefiro_unknown = 1\n")]
#[case::audio("audio", "[audio]\nzefiro_unknown = 1\n")]
#[case::server("server", "[[server]]\nzefiro_unknown = 1\n")]
#[case::cover("cover", "[cover]\nzefiro_unknown = 1\n")]
#[case::cover_cells("cover.cover_cells", "[cover.cover_cells]\nzefiro_unknown = 1\n")]
#[case::card("card", "[card]\nzefiro_unknown = 1\n")]
#[case::progress("progress", "[progress]\nzefiro_unknown = 1\n")]
#[case::layout("layout", "[layout]\nzefiro_unknown = 1\n")]
#[case::window("window", "[window]\nzefiro_unknown = 1\n")]
fn every_key_of_a_table_is_in_the_template(#[case] table: &str, #[case] probe: &str) {
    let error = parse_config(probe)
        .expect_err("an unknown key must not parse")
        .to_string();
    let (_, expected) = error.split_once("expected").unwrap();
    let fields: Vec<_> = expected.split('`').skip(1).step_by(2).collect();
    let keys = template_keys();

    assert!(!fields.is_empty(), "{probe}");
    let missing: Vec<_> = fields
        .iter()
        .filter(|field| !keys.contains(&(table.to_owned(), (**field).to_owned())))
        .collect();
    assert!(missing.is_empty(), "[{table}] lacks {missing:?}");
}

#[test]
fn a_patch_round_trips_through_the_public_parser() {
    let patch = ConfigPatch {
        theme_name: Some(ThemeName::from_static("noir")),
        volume: Some(Percent::clamped(42)),
        device: Some(OutputDevice::Named(
            DeviceName::new("Speakers".to_string()).unwrap(),
        )),
        ..ConfigPatch::default()
    };
    let written = patched_config_text(COMMENTED_CONFIG, patch).unwrap();
    let round_tripped = parse_config(&written).unwrap();
    assert_eq!(
        round_tripped.theme_choice,
        ThemeChoice::Named(ThemeName::from_static("noir"))
    );
    assert_eq!(round_tripped.volume, Percent::clamped(42));
    assert_eq!(
        AudioSettings::from(round_tripped.audio.clone()).device,
        OutputDevice::Named(DeviceName::new("Speakers".to_string()).unwrap())
    );
    insta::assert_debug_snapshot!(round_tripped);
}

#[test]
fn parse_config_settings_reads_the_keymap_and_the_music_dir() {
    let parsed = parse_config_settings(
        "music_dir = \"/tmp/music\"\ntheme = \"dark\"\n\n[keymap]\nnext = \"x\"\n",
    )
    .unwrap();
    insta::assert_debug_snapshot!(parsed);
}
