use kernel::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, Playback},
    domain::{
        cue::{Cue, PlaybackChange},
        model::Model,
        player::Player,
        revision::Revision,
        time::Moment,
        toast::TOAST_LIFETIME,
        transport::{Output, StreamError},
    },
    message::{AudioEvent, Message, PlaybackRequest, Timer},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{model_with_tracks, playing_model, update::update};

fn first_toast_expiry() -> Effect {
    Effect::After {
        delay: TOAST_LIFETIME,
        timer: Timer::Toast(Revision::default().next()),
    }
}

fn second_toast_expiry() -> Effect {
    Effect::After {
        delay: TOAST_LIFETIME,
        timer: Timer::Toast(Revision::default().next().next()),
    }
}

fn output_lost() -> Message {
    Message::Audio(AudioEvent::OutputLost(StreamError::DeviceGone))
}

fn lost_while_playing(count: usize) -> Model {
    let mut model = playing_model(count);
    let _lost = update(&mut model, output_lost(), Moment::default()).unwrap();
    model
}

#[rstest]
#[case::playing_pauses_and_says_so(
    playing_model(3),
    Cmd::from_iter([
        Effect::Animate(Cue::ToastRaised),
        second_toast_expiry(),
        Effect::Audio(AudioCmd::SetPlayback(Playback::Paused)),
        Effect::Macos(MacosCmd::SetPlayback(Playback::Paused)),
        Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Pause)),
    ]),
    "Output lost — paused"
)]
#[case::stopped_only_marks_the_output(
    model_with_tracks(3),
    Cmd::from_iter([
        Effect::Animate(Cue::ToastRaised),
        first_toast_expiry(),
    ]),
    "Audio output lost: the device is gone"
)]
fn a_lost_output_is_mirrored_in_the_model(
    #[case] mut model: Model,
    #[case] expected: Cmd,
    #[case] toast: &str,
) {
    let was_playing = model.player.is_playing();
    let cmd = update(&mut model, output_lost(), Moment::default()).unwrap();

    assert_eq!(cmd, expected);
    assert!(matches!(model.transport.output, Output::Lost(..)));
    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .and_then(|shown| shown.text.as_deref()),
        Some(toast)
    );
    if was_playing {
        assert!(matches!(model.player, Player::Paused { .. }));
    }
}

#[test]
fn a_lost_output_under_a_paused_player_leaves_it_paused_and_says_so() {
    let mut model = playing_model(3);
    let _paused = update(
        &mut model,
        Message::Playback(PlaybackRequest::Toggle),
        Moment::default(),
    )
    .unwrap();
    let paused = model.player.clone();

    let cmd = update(&mut model, output_lost(), Moment::default()).unwrap();

    assert_eq!(model.player, paused);
    assert!(matches!(model.player, Player::Paused { .. }));
    assert_eq!(
        model.transport.output,
        Output::Lost(StreamError::DeviceGone)
    );
    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Animate(Cue::ToastRaised)))
    );
    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .and_then(|shown| shown.text.as_deref()),
        Some("Output lost — paused")
    );
}

#[test]
fn play_while_the_output_is_lost_loads_again_so_the_engine_reopens() {
    let mut model = lost_while_playing(3);

    let cmd = update(
        &mut model,
        Message::Playback(PlaybackRequest::Play),
        Moment::default(),
    )
    .unwrap();

    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::Load(_)))),
        "expected a load among {cmd:?}"
    );
    assert!(matches!(model.player, Player::Loading(..)));
}

#[rstest]
#[case::toggle(PlaybackRequest::Toggle)]
#[case::play(PlaybackRequest::Play)]
fn playing_while_the_output_is_lost_with_no_track_is_refused(
    #[case] request: PlaybackRequest,
) {
    let mut model = model_with_tracks(0);
    model.transport.output = Output::Lost(StreamError::DeviceGone);

    let result = update(&mut model, Message::Playback(request), Moment::default());

    assert_eq!(result, Err(Unhandled));
    assert_eq!(model.player, Player::Stopped);
    assert_eq!(
        model.transport.output,
        Output::Lost(StreamError::DeviceGone)
    );
}

#[test]
fn a_track_that_loads_after_the_reopen_clears_the_lost_output() {
    let mut model = lost_while_playing(3);
    let _play = update(
        &mut model,
        Message::Playback(PlaybackRequest::Play),
        Moment::default(),
    )
    .unwrap();

    let _loaded = update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(model.transport.output, Output::Ready);
    assert!(matches!(model.player, Player::Playing { .. }));
}

#[test]
fn a_loss_repeated_after_the_retry_stops_the_player_again() {
    let mut model = lost_while_playing(3);
    let _play = update(
        &mut model,
        Message::Playback(PlaybackRequest::Play),
        Moment::default(),
    )
    .unwrap();

    let cmd = update(&mut model, output_lost(), Moment::default()).unwrap();

    assert_eq!(model.player, Player::Stopped);
    assert_eq!(
        model.transport.output,
        Output::Lost(StreamError::DeviceGone)
    );
    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Animate(Cue::ToastRaised)))
    );
}
