use std::time::Duration;

use kernel::{
    AudioEvent,
    Cmd,
    Cue,
    Effect,
    Message,
    Model,
    Moment,
    PlaybackChange,
    PlaybackRequest,
    Player,
    Timer,
    Toast,
    WorkspaceRequest,
    update::update,
};

use crate::support::playing_model;

fn sent(model: &mut Model, message: Message) -> Cmd {
    update(model, message, Moment::default()).unwrap()
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
    Message::Audio(AudioEvent::Playhead(at))
}

fn moment(millis: u64) -> Moment {
    Moment::new(Duration::from_millis(millis))
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

#[test]
fn a_stale_mark_is_ignored() {
    let mut model = playing_model(3);
    let armed = sent(&mut model, position(secs(50)));
    let stale = scheduled(&armed)[0];
    let _ = sent(&mut model, position(secs(60)));

    let cmd = sent(&mut model, Message::Elapsed(stale));

    assert_eq!(cmd, Cmd::None);
}

#[test]
fn played_for_accumulates_wall_time_across_playhead_reports() {
    let mut model = playing_model(3);
    let _ = update(&mut model, position(millis(100)), moment(100)).unwrap();
    let _ = update(&mut model, position(millis(200)), moment(250)).unwrap();
    assert_eq!(model.workspace.played_for, millis(250));
}

#[test]
fn played_for_keeps_accumulating_across_a_seek() {
    let mut model = playing_model(3);
    let _ = update(&mut model, position(millis(100)), moment(100)).unwrap();
    let _ = update(
        &mut model,
        Message::Playback(PlaybackRequest::SeekTo(secs(60))),
        moment(150),
    )
    .unwrap();
    let _ = update(&mut model, position(secs(60) + millis(50)), moment(400)).unwrap();
    assert_eq!(model.workspace.played_for, millis(400));
}

#[test]
fn played_for_keeps_the_wall_time_across_a_track_change() {
    let mut model = playing_model(3);
    let _ = update(&mut model, position(millis(100)), moment(100)).unwrap();
    let _ = update(
        &mut model,
        Message::Audio(AudioEvent::TrackChanged),
        moment(150),
    )
    .unwrap();
    let _ = update(&mut model, position(millis(50)), moment(300)).unwrap();
    assert_eq!(model.workspace.played_for, millis(300));
}
