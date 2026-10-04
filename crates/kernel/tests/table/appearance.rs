use kernel::{
    ConfigEvent,
    Message,
    Model,
    Moment,
    domain::{
        appearance::{
            Appearance,
            AppearanceSettings,
            Breakpoints,
            CoverMode,
            ProgressBar,
            Rgb,
        },
        geometry::{Cells, Pixels},
    },
};
use rstest::rstest;

use crate::support::step::update;

fn reloaded(appearance: Appearance) -> Model {
    let mut model = Model::default();
    let cmd = update(
        &mut model,
        Message::Config(ConfigEvent::AppearanceReloaded(appearance)),
        Moment::default(),
    )
    .unwrap();
    assert!(cmd.effects().next().is_none());
    model
}

#[rstest]
#[case::stock(Appearance::default())]
#[case::another_cover(Appearance {
    settings: AppearanceSettings { cover_mode: CoverMode::Off, ..AppearanceSettings::default() },
    cover_size_px: Pixels(320),
    ..Appearance::default()
})]
#[case::another_rules(Appearance {
    breakpoints: Breakpoints { min_columns: Cells(10), ..Breakpoints::default() },
    progress: ProgressBar { fill: Some(Rgb([1, 2, 3])), ..ProgressBar::default() },
    ..Appearance::default()
})]
fn a_reloaded_appearance_replaces_the_appearance(#[case] appearance: Appearance) {
    assert_eq!(reloaded(appearance).settings.appearance, appearance);
}
