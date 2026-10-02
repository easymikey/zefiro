use kernel::{
    ConfigEvent,
    Message,
    Model,
    Moment,
    domain::appearance::{Appearance, Breakpoints, CoverStyle, Look, ProgressBar, Rgb},
    update::update,
};
use rstest::rstest;

fn reloaded(look: Look) -> Model {
    let mut model = Model::default();
    let cmd = update(
        &mut model,
        Message::Config(ConfigEvent::AppearanceReloaded(look)),
        Moment::default(),
    )
    .unwrap();
    assert!(cmd.effects().next().is_none());
    model
}

#[rstest]
#[case::stock(Look::default())]
#[case::another_cover(Look {
    appearance: Appearance { cover_style: CoverStyle::Off, ..Appearance::default() },
    cover_size_px: 320,
    ..Look::default()
})]
#[case::another_rules(Look {
    breakpoints: Breakpoints { min_columns: 10, ..Breakpoints::default() },
    progress: ProgressBar { fill: Some(Rgb([1, 2, 3])), ..ProgressBar::default() },
    ..Look::default()
})]
fn a_reloaded_appearance_replaces_the_look(#[case] look: Look) {
    assert_eq!(reloaded(look).settings.look, look);
}
