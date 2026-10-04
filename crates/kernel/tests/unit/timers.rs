use std::time::Duration;

use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        cue::{Cue, PlaybackChange},
        model::Model,
        player::Player,
        time::Moment,
        toast::Toast,
    },
    message::{AudioEvent, Message, PlaybackRequest, Timer},
    update::machine::Unhandled,
};

use crate::support::{
    playing_model,
    update::{send_at, update},
};

fn sent(model: &mut Model, message: Message) -> Cmd {
    update(model, message, Moment::default()).unwrap()
}

fn sent_at(model: &mut Model, message: Message, at: Moment) -> Cmd {
    update(model, message, at).unwrap()
}

fn scheduled(cmd: &Cmd) -> Vec<Timer> {
    cmd.effects()
        .filter_map(|effect| {
            if let Effect::After { timer: message, .. } = effect {
                Some(*message)
            } else {
                None
            }
        })
        .collect()
}

fn toast_shown(model: &mut Model, text: &str) -> Timer {
    let cmd = sent(model, Message::Toast(Toast::info(text)));
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

fn millis(count: u64) -> Duration {
    Duration::from_millis(count)
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

    let cmd = sent_at(&mut model, Message::Elapsed(timer), moment(5000));

    assert_eq!(cmd, Cmd::from(Cue::ToastDismissed));
    assert!(model.workspace.toasts.is_empty());
}

#[test]
fn a_toast_timer_that_fires_early_keeps_the_toast_and_waits_again() {
    let mut model = Model::default();
    let timer = toast_shown(&mut model, "hello");

    let cmd = sent_at(&mut model, Message::Elapsed(timer), moment(4000));

    assert_eq!(
        cmd,
        Cmd::from(Effect::After {
            delay: secs(1),
            timer,
        })
    );
    assert_eq!(model.workspace.toasts.len(), 1);
}

#[test]
fn a_later_toast_shares_the_timer_and_expires_by_its_own_age() {
    let mut model = Model::default();
    let timer = toast_shown(&mut model, "first");
    let second = sent_at(
        &mut model,
        Message::Toast(Toast::info("second")),
        moment(3000),
    );
    assert_eq!(second, Cmd::from(Cue::ToastRaised));

    let first_gone = sent_at(&mut model, Message::Elapsed(timer), moment(5000));
    let titles: Vec<&str> = model
        .workspace
        .toasts
        .iter()
        .map(|toast| toast.title.as_str())
        .collect();
    assert_eq!(titles, ["second"]);
    assert_eq!(
        first_gone,
        Cmd::from_iter([
            Effect::Animate(Cue::ToastDismissed),
            Effect::After {
                delay: secs(3),
                timer,
            },
        ])
    );

    let second_gone = sent_at(&mut model, Message::Elapsed(timer), moment(8000));
    assert_eq!(second_gone, Cmd::from(Cue::ToastDismissed));
    assert!(model.workspace.toasts.is_empty());
}

#[test]
fn a_stale_toast_timer_changes_nothing() {
    let mut model = Model::default();
    let first = toast_shown(&mut model, "first");
    model.workspace.toasts.clear();
    let second = toast_shown(&mut model, "second");

    let stale = update(&mut model, Message::Elapsed(first), moment(9000));

    assert_ne!(first, second);
    assert_eq!(stale, Err(Unhandled));
    assert_eq!(model.workspace.toasts.len(), 1);
}

#[test]
fn arming_the_sleep_timer_schedules_the_first_preset() {
    let mut model = playing_model(3);
    let first_preset = model.settings.audio.sleep_presets.as_slice()[0];

    let cmd = sleep_cycled(&mut model);

    assert_eq!(
        cmd,
        Cmd::effect(Effect::After {
            delay: first_preset,
            timer: Timer::Sleep(model.revisions.sleep),
        })
    );
}

#[test]
fn an_elapsed_sleep_timer_pauses_in_place_and_disarms() {
    let mut model = playing_model(3);
    let timer = scheduled(&sleep_cycled(&mut model))[0];

    let cmd = sent(&mut model, Message::Elapsed(timer));
    let again = update(&mut model, Message::Elapsed(timer), Moment::default());

    assert_eq!(cmd, PlaybackChange::Pause.cued());
    assert!(matches!(model.player, Player::Paused { .. }));
    assert_eq!(model.transport.sleep, None);
    assert_eq!(again, Err(Unhandled));
}

#[test]
fn a_rearmed_sleep_timer_ignores_the_first_one() {
    let mut model = playing_model(3);
    let first = scheduled(&sleep_cycled(&mut model))[0];
    let _second = sleep_cycled(&mut model);

    let cmd = update(&mut model, Message::Elapsed(first), Moment::default());

    assert_eq!(cmd, Err(Unhandled));
    assert!(model.player.is_playing());
    assert_eq!(
        model.transport.sleep.map(|timer| timer.preset_index.get()),
        Some(1)
    );
}

#[test]
fn a_cancelled_sleep_timer_changes_nothing() {
    let mut model = playing_model(3);
    let presets = model.settings.audio.sleep_presets.as_slice().len();
    let armed: Vec<Timer> = (0..presets)
        .flat_map(|_| scheduled(&sleep_cycled(&mut model)))
        .collect();
    let cancelled = sleep_cycled(&mut model);

    let last = armed.last().copied().unwrap();
    let cmd = update(&mut model, Message::Elapsed(last), Moment::default());

    assert_eq!(cancelled, Cmd::none());
    assert_eq!(cmd, Err(Unhandled));
    assert!(model.player.is_playing());
    assert_eq!(model.transport.sleep, None);
}

#[test]
fn a_stale_mark_is_ignored() {
    let mut model = playing_model(3);
    let armed = sent(&mut model, position(secs(50)));
    let stale = scheduled(&armed)[0];
    drop(sent(&mut model, position(secs(60))));

    let cmd = update(&mut model, Message::Elapsed(stale), Moment::default());

    assert_eq!(cmd, Err(Unhandled));
}

#[test]
fn played_for_accumulates_wall_time_across_playhead_reports() {
    let mut model = playing_model(3);
    send_at(&mut model, position(millis(100)), moment(100));
    send_at(&mut model, position(millis(200)), moment(250));
    assert_eq!(model.workspace.played_for, millis(250));
}

#[test]
fn played_for_keeps_accumulating_across_a_seek() {
    let mut model = playing_model(3);
    send_at(&mut model, position(millis(100)), moment(100));
    send_at(
        &mut model,
        Message::Playback(PlaybackRequest::SeekTo(secs(60))),
        moment(150),
    );
    send_at(&mut model, position(secs(60) + millis(50)), moment(400));
    assert_eq!(model.workspace.played_for, millis(400));
}

#[test]
fn played_for_keeps_the_wall_time_across_a_track_change() {
    let mut model = playing_model(3);
    send_at(&mut model, position(millis(100)), moment(100));
    send_at(
        &mut model,
        Message::Audio(AudioEvent::TrackChanged),
        moment(150),
    );
    send_at(&mut model, position(millis(50)), moment(300));
    assert_eq!(model.workspace.played_for, millis(300));
}
