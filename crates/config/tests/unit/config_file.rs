use config::{
    config_file::{parse_config, parse_config_settings},
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

const COMMENTED_CONFIG: &str = include_str!("../fixtures/config_commented.toml");

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
