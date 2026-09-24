use config::{parse, parse_keymap, patched};
use kernel::{Bounded, ConfigPatch, DevicePatch, domain::Percent};

const COMMENTED_CONFIG: &str = include_str!("../fixtures/config_commented.toml");

#[test]
fn the_commented_fixture_parses_into_every_table() {
    let parsed = parse(COMMENTED_CONFIG).unwrap();
    insta::assert_debug_snapshot!(parsed);
}

#[test]
fn a_patch_round_trips_through_the_public_parser() {
    let patch = ConfigPatch::builder()
        .theme("noir")
        .volume(Percent::clamped(42))
        .device(DevicePatch::Named("Speakers".into()))
        .build();
    let written = patched(COMMENTED_CONFIG, patch).unwrap();
    let round_tripped = parse(&written).unwrap();
    insta::assert_debug_snapshot!(round_tripped);
}

#[test]
fn parse_keymap_reads_the_keymap_and_the_root_music_dir() {
    let parsed = parse_keymap(
        "music_dir = \"/tmp/music\"\ntheme = \"dark\"\n\n[keymap]\nnext = \"x\"\n",
    )
    .unwrap();
    insta::assert_debug_snapshot!(parsed);
}
