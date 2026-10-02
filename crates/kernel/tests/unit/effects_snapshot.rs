use kernel::{
    AudioEvent,
    BrowseRequest,
    ConfigEvent,
    LibraryEvent,
    Message,
    Model,
    Moment,
    PlaybackRequest,
    domain::{
        AppearanceControl,
        AppearanceRow,
        AppearanceSetting,
        Choice,
        Direction,
        OptionCount,
        Revision,
        SettingRow,
        ThemeName,
        appearance_rows::AppearanceField,
    },
    update::update,
};

use crate::support::{effects, model_with_tracks, playing_model};

#[test]
fn toggling_from_stopped_starts_the_track() {
    let mut m = model_with_tracks(3);
    let cmd = update(
        &mut m,
        Message::Playback(PlaybackRequest::Toggle),
        Moment::default(),
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn play_selected_emits_its_effects() {
    let mut m = model_with_tracks(3);
    let cmd = update(
        &mut m,
        Message::Browse(BrowseRequest::PlaySelected),
        Moment::default(),
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn adjusting_a_custom_row_emits_its_effect() {
    let mut m = Model::default();
    let id = AppearanceField::SpeedChip;
    let count = OptionCount::new(4).unwrap();
    let row: &'static AppearanceRow = Box::leak(Box::new(AppearanceRow {
        field: id,
        control: AppearanceControl::Cycle(count),
        cue: None,
        themes: &[],
    }));
    m.appearance_settings.push(AppearanceSetting {
        row,
        choice: Choice::Option(count.index(0).unwrap()),
    });
    let cmd = update(
        &mut m,
        Message::Adjust {
            row: SettingRow::Appearance(id),
            direction: Direction::Next,
        },
        Moment::default(),
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn theme_reloaded_emits_its_effect() {
    let mut m = Model::default();
    let cmd = update(
        &mut m,
        Message::Config(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir"))),
        Moment::default(),
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn library_loaded_emits_its_effects() {
    let mut m = Model::default();
    let cmd = update(
        &mut m,
        Message::Library(LibraryEvent::Loaded {
            tracks: vec![],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn a_track_ending_auto_advances_to_the_next_one() {
    let mut m = playing_model(3);
    let cmd =
        update(&mut m, Message::Audio(AudioEvent::Ended), Moment::default()).unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}
