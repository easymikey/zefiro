use config::{AppearancePatch, CoverStyle, appearance_patched, parse_appearance};

const COMMENTED_UI: &str = include_str!("../fixtures/sifr-ui_commented.toml");

#[test]
fn the_commented_fixture_parses_into_every_table() {
    let parsed = parse_appearance(COMMENTED_UI).unwrap();
    insta::assert_debug_snapshot!(parsed);
}

#[test]
fn a_patch_round_trips_through_the_public_parser() {
    let patch = AppearancePatch::builder()
        .cover_style(CoverStyle::Milkdrop)
        .build();
    let written = appearance_patched(COMMENTED_UI, patch).unwrap();
    let round_tripped = parse_appearance(&written).unwrap();
    insta::assert_debug_snapshot!(round_tripped);
}
