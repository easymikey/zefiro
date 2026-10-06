use kernel::{
    domain::{
        appearance::{AppearanceSettings, CoverMode},
        model::Model,
        time::Moment,
    },
    message::{ConfigEvent, Message},
};
use rstest::rstest;

use crate::support::update::update;

fn reloaded(appearance_settings: AppearanceSettings) -> Model {
    let mut model = Model::default();
    let cmd = update(
        &mut model,
        Message::Config(ConfigEvent::AppearanceReloaded(appearance_settings)),
        Moment::default(),
    )
    .unwrap();
    assert!(cmd.effects().next().is_none());
    model
}

#[rstest]
#[case::stock(AppearanceSettings::default())]
#[case::another_cover(AppearanceSettings {
    cover_mode: CoverMode::Off,
    ..AppearanceSettings::default()
})]
fn a_reloaded_appearance_replaces_the_appearance(
    #[case] appearance_settings: AppearanceSettings,
) {
    assert_eq!(
        reloaded(appearance_settings).settings.appearance_settings,
        appearance_settings
    );
}
