use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioEvent,
    Cmd,
    Effect,
    Message,
    Model,
    PlaybackRequest,
    Preload,
    update::update,
};

use crate::support::model_with_dated_tracks;

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn preloaded(cmd: &Cmd) -> Option<PathBuf> {
    cmd.effects().find_map(preload_path)
}

fn preload_path(effect: &Effect) -> Option<PathBuf> {
    if let Effect::Audio(AudioCmd::Preload { path, .. }) = effect {
        return Some(path.clone());
    }
    None
}

fn loaded(cmd: &Cmd) -> Option<PathBuf> {
    cmd.effects().find_map(load_path)
}

fn load_path(effect: &Effect) -> Option<PathBuf> {
    if let Effect::Audio(AudioCmd::Load { path, .. }) = effect {
        return Some(path.clone());
    }
    None
}

fn preload_revision(effect: &Effect) -> Option<kernel::domain::Revision> {
    if let Effect::Audio(AudioCmd::Preload { revision, .. }) = effect {
        return Some(*revision);
    }
    None
}

fn playing_three() -> Model {
    let mut model = model_with_dated_tracks(3);
    let _ = update(&mut model, Message::Playback(PlaybackRequest::Toggle)).unwrap();
    let _ = update(
        &mut model,
        Message::Audio(AudioEvent::Loaded { total: None }),
    )
    .unwrap();
    model
}

#[test]
fn a_tick_near_the_end_arms_the_preload() {
    let mut model = playing_three();

    let early =
        update(&mut model, Message::Audio(AudioEvent::Position(secs(50)))).unwrap();
    assert_eq!(preloaded(&early), None);

    let late =
        update(&mut model, Message::Audio(AudioEvent::Position(secs(95)))).unwrap();
    assert_eq!(preloaded(&late), Some(PathBuf::from("/tmp/track1.flac")));
    assert!(matches!(
        model.player,
        kernel::Player::Playing {
            preload: Preload::Queued(_),
            ..
        }
    ));
}

#[test]
fn the_armed_preload_is_stamped_fresh() {
    let mut model = playing_three();
    let cmd =
        update(&mut model, Message::Audio(AudioEvent::Position(secs(95)))).unwrap();
    let revision = cmd.effects().find_map(preload_revision);
    assert!(
        revision
            .is_some_and(|revision| revision != kernel::domain::Revision::UNSTAMPED)
    );
}

#[test]
fn the_hand_off_adopts_the_preloaded_track_without_a_second_load() {
    let mut model = playing_three();
    let _ = update(&mut model, Message::Audio(AudioEvent::Position(secs(95)))).unwrap();

    let cmd = update(&mut model, Message::Audio(AudioEvent::TrackChanged)).unwrap();

    assert_eq!(loaded(&cmd), None);
    assert_eq!(
        model
            .player
            .current()
            .map(|track| track.path().to_path_buf()),
        Some(PathBuf::from("/tmp/track1.flac"))
    );
}
