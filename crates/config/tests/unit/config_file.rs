use config::{
    appearance_file::parse_appearance,
    config_file::{parse_config, parse_config_settings},
    patch::patched_config_text,
    theme_file::parse_theme,
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

const COMMENTED_CONFIG: &str = include_str!("../fixtures/config_commented.toml");

#[test]
fn the_commented_fixture_parses_into_every_table() {
    let parsed = parse_config(COMMENTED_CONFIG).unwrap();
    insta::assert_debug_snapshot!(parsed);
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

const THEME_KEYS: [&str; 7] = [
    "background",
    "muted_foreground",
    "foreground",
    "accent",
    "green",
    "yellow",
    "red",
];

fn theme_with(new_key: &str, key: &str) -> String {
    let colors: String = THEME_KEYS
        .iter()
        .filter(|theme_key| **theme_key != new_key)
        .map(|theme_key| format!("{theme_key} = \"#102030\"\n"))
        .collect();
    format!("name = \"mine\"\n[colors]\n{colors}{key} = \"#a0b0c0\"\n")
}

fn parsed_config(text: &str) -> String {
    format!("{:?}", parse_config(text))
}

fn parsed_appearance(text: &str) -> String {
    format!("{:?}", parse_appearance(text))
}

fn parsed_theme(text: &str) -> String {
    format!("{:?}", parse_theme(text, "mine"))
}

#[rstest]
#[case::replaygain(
    "[audio]\nreplaygain = true\n".to_owned(),
    "[audio]\nreplay_gain = true\n".to_owned(),
    parsed_config
)]
#[case::text_cells(
    "[cover.text_cells]\nwidth = 30\nheight = 10\n".to_owned(),
    "[cover.cover_cells]\nwidth = 30\nheight = 10\n".to_owned(),
    parsed_appearance
)]
#[case::min_columns(
    "[layout]\nmin_columns = 50\n".to_owned(),
    "[layout]\nmin_width = 50\n".to_owned(),
    parsed_appearance
)]
#[case::min_rows(
    "[layout]\nmin_rows = 12\n".to_owned(),
    "[layout]\nmin_height = 12\n".to_owned(),
    parsed_appearance
)]
#[case::bg(
    theme_with("background", "bg"),
    theme_with("background", "background"),
    parsed_theme
)]
#[case::fg(
    theme_with("muted_foreground", "fg"),
    theme_with("muted_foreground", "muted_foreground"),
    parsed_theme
)]
#[case::bright_fg(
    theme_with("foreground", "bright_fg"),
    theme_with("foreground", "foreground"),
    parsed_theme
)]
#[case::window_bg(
    theme_with("window_background", "window_bg"),
    theme_with("window_background", "window_background"),
    parsed_theme
)]
fn an_old_key_parses_like_its_new_name(
    #[case] old_text: String,
    #[case] new_text: String,
    #[case] parsed: fn(&str) -> String,
) {
    let new_parsed = parsed(&new_text);

    assert!(new_parsed.starts_with("Ok("), "{new_parsed}");
    assert_eq!(parsed(&old_text), new_parsed);
}
