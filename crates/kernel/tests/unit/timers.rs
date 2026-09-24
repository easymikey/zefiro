use std::time::Duration;

use kernel::{
    AudioEvent,
    Cmd,
    Cue,
    Effect,
    Message,
    Model,
    PlaybackChange,
    PlaybackRequest,
    Player,
    Timer,
    Toast,
    WorkspaceRequest,
    update::update,
};
use rstest::rstest;

use crate::support::playing_model;

fn sent(model: &mut Model, message: Message) -> Cmd {
    update(model, message).unwrap()
}

fn scheduled(cmd: &Cmd) -> Vec<Timer> {
    cmd.effects()
        .filter_map(|effect| {
            if let Effect::After { message, .. } = effect {
                Some(*message)
            } else {
                None
            }
        })
        .collect()
}

fn toast_shown(model: &mut Model, text: &str) -> Timer {
    let cmd = sent(
        model,
        Message::Workspace(WorkspaceRequest::ShowToast(Toast::info(text.to_string()))),
    );
    let timers = scheduled(&cmd);
    assert!(matches!(timers.as_slice(), [Timer::Toast(_)]), "{timers:?}");
    timers[0]
}

fn sleep_cycled(model: &mut Model) -> Cmd {
    sent(model, Message::Playback(PlaybackRequest::CycleSleep))
}

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn millis(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn position(at: Duration) -> Message {
    Message::Audio(AudioEvent::Position(at))
}

#[test]
fn an_elapsed_toast_timer_takes_the_toast_down() {
    let mut model = Model::default();
    let timer = toast_shown(&mut model, "hello");

    let cmd = sent(&mut model, Message::Elapsed(timer));

    assert_eq!(cmd, Cmd::from(Cue::ToastDismissed));
    assert!(model.workspace.toast.is_none());
}

#[test]
fn a_replaced_toast_outlives_the_first_timer() {
    let mut model = Model::default();
    let first = toast_shown(&mut model, "first");
    let second = toast_shown(&mut model, "second");

    let stale = sent(&mut model, Message::Elapsed(first));
    let shown = model.workspace.toast.clone().map(|toast| toast.text);
    let expired = sent(&mut model, Message::Elapsed(second));

    assert_eq!(stale, Cmd::None);
    assert_eq!(shown.as_deref(), Some("second"));
    assert_eq!(expired, Cmd::from(Cue::ToastDismissed));
    assert!(model.workspace.toast.is_none());
}

#[test]
fn arming_the_sleep_timer_schedules_the_first_preset() {
    let mut model = playing_model(3);
    let first_preset = model.settings.sleep_presets[0];

    let cmd = sleep_cycled(&mut model);

    assert_eq!(
        cmd,
        Cmd::One(Effect::After {
            delay: first_preset,
            message: Timer::Sleep(model.sleep_generation),
        })
    );
}

#[test]
fn an_elapsed_sleep_timer_pauses_in_place_and_disarms() {
    let mut model = playing_model(3);
    let timer = scheduled(&sleep_cycled(&mut model))[0];

    let cmd = sent(&mut model, Message::Elapsed(timer));
    let again = sent(&mut model, Message::Elapsed(timer));

    assert_eq!(cmd, PlaybackChange::Pause.cued());
    assert!(matches!(model.player, Player::Paused { .. }));
    assert_eq!(model.transport.sleep, None);
    assert_eq!(again, Cmd::None);
}

#[test]
fn a_rearmed_sleep_timer_ignores_the_first_one() {
    let mut model = playing_model(3);
    let first = scheduled(&sleep_cycled(&mut model))[0];
    let _second = sleep_cycled(&mut model);

    let cmd = sent(&mut model, Message::Elapsed(first));

    assert_eq!(cmd, Cmd::None);
    assert!(model.player.is_playing());
    assert_eq!(
        model.transport.sleep.map(|timer| timer.preset_index),
        Some(1)
    );
}

#[test]
fn a_cancelled_sleep_timer_changes_nothing() {
    let mut model = playing_model(3);
    let presets = model.settings.sleep_presets.len();
    let armed: Vec<Timer> = (0..presets)
        .flat_map(|_| scheduled(&sleep_cycled(&mut model)))
        .collect();
    let cancelled = sleep_cycled(&mut model);

    let last = armed.last().copied().unwrap();
    let cmd = sent(&mut model, Message::Elapsed(last));

    assert_eq!(cancelled, Cmd::None);
    assert_eq!(cmd, Cmd::None);
    assert!(model.player.is_playing());
    assert_eq!(model.transport.sleep, None);
}

#[rstest]
#[case::steady_positions_add_up(
    vec![position(millis(100)), position(millis(200)), position(millis(300))],
    millis(300)
)]
#[case::a_seek_is_not_listening(
    vec![
        position(millis(500)),
        Message::Playback(PlaybackRequest::SeekTo(secs(60))),
        position(secs(60) + millis(100)),
    ],
    millis(600)
)]
#[case::a_stale_position_racing_a_seek_is_not_listening(
    vec![
        position(millis(100)),
        Message::Playback(PlaybackRequest::SeekTo(secs(60))),
        position(millis(200)),
        position(secs(60) + millis(100)),
    ],
    millis(100)
)]
#[case::a_track_change_starts_counting_from_zero(
    vec![position(millis(500)), Message::Audio(AudioEvent::TrackChanged), position(millis(100))],
    millis(600)
)]
fn played_for_follows_the_positions_audio_reports(
    #[case] messages: Vec<Message>,
    #[case] played_for: Duration,
) {
    let mut model = playing_model(3);

    for message in messages {
        let _cmd = sent(&mut model, message);
    }

    assert_eq!(model.workspace.played_for, played_for);
}
