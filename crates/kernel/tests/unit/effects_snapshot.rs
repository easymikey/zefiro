use kernel::{
    AudioEvent,
    BrowseRequest,
    LoadedRequest,
    Message,
    Model,
    PlaybackRequest,
    WorkspaceRequest,
    domain::{CustomSetting, Nudge, Revision, SettingControl, SettingId, SettingRow},
    update::update,
};

use crate::support::{effects, model_with_tracks, playing_model};

#[test]
fn toggling_from_stopped_starts_the_track() {
    let mut m = model_with_tracks(3);
    let cmd = update(&mut m, Message::Playback(PlaybackRequest::Toggle)).unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn play_selected_emits_its_effects() {
    let mut m = model_with_tracks(3);
    let cmd = update(&mut m, Message::Browse(BrowseRequest::PlaySelected)).unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn adjusting_a_custom_row_emits_its_effect() {
    let mut m = Model::default();
    let id = SettingId(3);
    m.custom_rows.push(CustomSetting {
        id,
        control: SettingControl::Cycle(4),
        position: 0,
        cue: None,
        themes: &[],
    });
    let cmd = update(
        &mut m,
        Message::Adjust {
            row: SettingRow::Custom(id),
            nudge: Nudge::Up,
        },
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn theme_reloaded_emits_its_effect() {
    let mut m = Model::default();
    let cmd =
        update(&mut m, Message::Workspace(WorkspaceRequest::ThemeReloaded)).unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn library_loaded_emits_its_effects() {
    let mut m = Model::default();
    let cmd = update(
        &mut m,
        Message::Loaded(LoadedRequest::LibraryLoaded {
            tracks: vec![],
            revision: Revision::default(),
        }),
    )
    .unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}

#[test]
fn a_track_ending_auto_advances_to_the_next_one() {
    let mut m = playing_model(3);
    let cmd = update(&mut m, Message::Audio(AudioEvent::Ended)).unwrap();
    insta::assert_debug_snapshot!(effects(cmd));
}
