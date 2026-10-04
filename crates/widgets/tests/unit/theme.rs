use config::parse_theme;
use rstest::rstest;
use widgets::{Colors, ThemeBase};

#[rstest]
#[case::terracotta_dark(
    "terracotta-dark",
    include_str!("../../../../themes/terracotta-dark.toml")
)]
#[case::terracotta_light(
    "terracotta-light",
    include_str!("../../../../themes/terracotta-light.toml")
)]
#[case::ember("ember", include_str!("../../../../themes/ember.toml"))]
#[case::gruvbox("gruvbox", include_str!("../../../../themes/gruvbox.toml"))]
#[case::gruvbox_light(
    "gruvbox-light",
    include_str!("../../../../themes/gruvbox-light.toml")
)]
#[case::hacker("hacker", include_str!("../../../../themes/hacker.toml"))]
#[case::macaroon("macaroon", include_str!("../../../../themes/macaroon.toml"))]
#[case::neobrutalism_dark(
    "neobrutalism-dark",
    include_str!("../../../../themes/neobrutalism-dark.toml")
)]
#[case::neobrutalism_light(
    "neobrutalism-light",
    include_str!("../../../../themes/neobrutalism-light.toml")
)]
#[case::noir("noir", include_str!("../../../../themes/noir.toml"))]
#[case::oreo("oreo", include_str!("../../../../themes/oreo.toml"))]
#[case::ristretto("ristretto", include_str!("../../../../themes/ristretto.toml"))]
#[case::rose_pine("rose-pine", include_str!("../../../../themes/rose-pine.toml"))]
#[case::rose_pine_dawn(
    "rose-pine-dawn",
    include_str!("../../../../themes/rose-pine-dawn.toml")
)]
#[case::wafer("wafer", include_str!("../../../../themes/wafer.toml"))]
#[case::winamp("winamp", include_str!("../../../../themes/winamp.toml"))]
fn every_repo_theme_derives_its_own_palette(#[case] name: &str, #[case] source: &str) {
    let file = parse_theme(source, name).unwrap();
    let c = file.colors;
    let colors = Colors::derive(&ThemeBase {
        background: c.background,
        foreground: c.foreground,
        bright_foreground: c.bright_foreground,
        accent: c.accent,
        green: c.green,
        yellow: c.yellow,
        red: c.red,
        window_background: c.window_background,
    });

    insta::with_settings!({ snapshot_suffix => name }, {
        insta::assert_debug_snapshot!(colors);
    });
}
