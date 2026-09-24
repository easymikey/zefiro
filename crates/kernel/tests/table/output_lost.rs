use kernel::{
    AudioCmd,
    Cmd,
    Cue,
    Effect,
    Message,
    Model,
    Playback,
    PlaybackChange,
    PlaybackRequest,
    Player,
    SystemCmd,
    domain::Output,
    message::{AudioEvent, AudioFailure},
    update::update,
};
use rstest::rstest;

use crate::support::{first_toast_expiry, model_with_tracks, playing_model};

fn output_lost() -> Message {
    Message::Audio(AudioEvent::Error(AudioFailure::OutputLost {
        reason: "device went away".to_string(),
    }))
}

fn lost_while_playing(count: usize) -> Model {
    let mut model = playing_model(count);
    let _lost = update(&mut model, output_lost()).unwrap();
    model
}

#[rstest]
#[case::playing_pauses_and_says_so(
    playing_model(3),
    Cmd::Batch(vec![
        Effect::Animate(Cue::ToastRaised),
        first_toast_expiry(),
        Effect::Audio(AudioCmd::Pause(Playback::Paused)),
        Effect::System(SystemCmd::PlaybackState(Playback::Paused)),
        Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Pause)),
    ]),
    "Output lost — paused"
)]
#[case::stopped_only_marks_the_output(
    model_with_tracks(3),
    Cmd::Batch(vec![Effect::Animate(Cue::ToastRaised), first_toast_expiry()]),
    "audio output stopped: device went away"
)]
fn a_lost_output_is_mirrored_in_the_model(
    #[case] mut model: Model,
    #[case] expected: Cmd,
    #[case] toast: &str,
) {
    let was_playing = model.player.is_playing();
    let cmd = update(&mut model, output_lost()).unwrap();

    assert_eq!(cmd, expected);
    assert!(matches!(model.transport.output, Output::Lost { .. }));
    assert_eq!(
        model.workspace.toast.map(|shown| shown.text).as_deref(),
        Some(toast)
    );
    if was_playing {
        assert!(matches!(model.player, Player::Paused { .. }));
    }
}

#[test]
fn play_while_the_output_is_lost_loads_again_so_the_engine_reopens() {
    let mut model = lost_while_playing(3);

    let cmd = update(&mut model, Message::Playback(PlaybackRequest::Play)).unwrap();

    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::Load { .. }))),
        "expected a load among {cmd:?}"
    );
    assert!(matches!(model.player, Player::Loading { .. }));
}

#[test]
fn a_track_that_loads_after_the_reopen_clears_the_lost_output() {
    let mut model = lost_while_playing(3);
    let _play = update(&mut model, Message::Playback(PlaybackRequest::Play)).unwrap();

    let _loaded = update(
        &mut model,
        Message::Audio(AudioEvent::Loaded { total: None }),
    )
    .unwrap();

    assert_eq!(model.transport.output, Output::Ready);
    assert!(matches!(model.player, Player::Playing { .. }));
}
