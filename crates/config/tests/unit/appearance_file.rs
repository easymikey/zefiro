use config::{appearance_file::parse_appearance, patch::patched_appearance_text};
use kernel::domain::appearance::{AppearancePatch, CoverMode};

const COMMENTED_UI: &str = include_str!("../fixtures/sifr-ui_commented.toml");

#[test]
fn a_patch_round_trips_through_the_public_parser() {
    let patch = AppearancePatch {
        cover_mode: Some(CoverMode::Milkdrop),
        ..AppearancePatch::default()
    };
    let written = patched_appearance_text(COMMENTED_UI, patch).unwrap();
    let round_tripped = parse_appearance(&written).unwrap();
    assert_eq!(
        round_tripped.to_appearance_settings().cover_mode,
        CoverMode::Milkdrop
    );
    insta::assert_debug_snapshot!(round_tripped);
}
