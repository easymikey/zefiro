use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioCmd,
    AudioEvent,
    Cmd,
    Effect,
    Message,
    Model,
    Moment,
    PlaybackRequest,
    Preload,
    Timer,
    TrackLoad,
};

use crate::support::{
    model_with_dated_tracks,
    step::{apply, update},
};

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn preloaded(cmd: &Cmd) -> Option<PathBuf> {
    cmd.effects().find_map(preload_path)
}

fn preload_path(effect: &Effect) -> Option<PathBuf> {
    if let Effect::Audio(AudioCmd::Preload(TrackLoad { path, .. })) = effect {
        return Some(path.clone());
    }
    None
}

fn loaded(cmd: &Cmd) -> Option<PathBuf> {
    cmd.effects().find_map(load_path)
}

fn load_path(effect: &Effect) -> Option<PathBuf> {
    if let Effect::Audio(AudioCmd::Load(TrackLoad { path, .. })) = effect {
        return Some(path.clone());
    }
    None
}

fn preload_revision(effect: &Effect) -> Option<kernel::domain::Revision> {
    if let Effect::Audio(AudioCmd::Preload(TrackLoad { revision, .. })) = effect {
        return Some(*revision);
    }
    None
}

fn playing_three() -> Model {
    let mut model = model_with_dated_tracks(3);
    apply(&mut model, Message::Playback(PlaybackRequest::Toggle));
    apply(&mut model, Message::Audio(AudioEvent::Loaded(None)));
    model
}

#[test]
fn a_tick_near_the_end_arms_the_preload() {
    let mut model = playing_three();

    apply(&mut model, Message::Audio(AudioEvent::Playhead(secs(50))));
    let first_mark = model.revisions.lookahead;
    let early = update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(first_mark)),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(preloaded(&early), None);

    apply(&mut model, Message::Audio(AudioEvent::Playhead(secs(95))));
    let second_mark = model.revisions.lookahead;
    let late = update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(second_mark)),
        Moment::default(),
    )
    .unwrap();
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
    apply(&mut model, Message::Audio(AudioEvent::Playhead(secs(95))));
    let mark = model.revisions.lookahead;
    let cmd = update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    )
    .unwrap();
    let revision = cmd.effects().find_map(preload_revision);
    assert!(
        revision
            .is_some_and(|revision| revision != kernel::domain::Revision::default())
    );
}

#[test]
fn the_hand_off_adopts_the_preloaded_track_without_a_second_load() {
    let mut model = playing_three();
    apply(&mut model, Message::Audio(AudioEvent::Playhead(secs(95))));
    let mark = model.revisions.lookahead;
    apply(&mut model, Message::Elapsed(Timer::Lookahead(mark)));

    let cmd = update(
        &mut model,
        Message::Audio(AudioEvent::TrackChanged),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(loaded(&cmd), None);
    assert_eq!(
        model
            .player
            .current()
            .map(|track| track.path().to_path_buf()),
        Some(PathBuf::from("/tmp/track1.flac"))
    );
}
