use std::time::Duration;

use kernel::{
    cmd::{AudioCmd, Cmd, Effect, LibraryCmd, Playback, TrackLoad},
    domain::{
        cue::Cue,
        driver::{DriverError, DriverName, DriverStatus},
        model::Model,
        player::{PausedBy, Player},
        playhead::Playhead,
        speed::Speed,
        time::Moment,
        toast::ToastLevel,
    },
    message::{DriverEvent, Message},
    update::machine::Unhandled,
};

use crate::support::{
    bare_track,
    dated_track,
    first_toast_expiry,
    playing_model,
    update::update,
};

fn died(driver_name: DriverName) -> Message {
    Message::Driver {
        driver_name,
        event: DriverEvent::Died(DriverError::Panicked),
    }
}

#[test]
fn a_driver_death_is_recorded_and_told_as_an_error() {
    let mut model = playing_model(3);

    let cmd = update(&mut model, died(DriverName::Config), Moment::default()).unwrap();

    assert_eq!(
        cmd,
        Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()])
    );

    assert_eq!(
        model.drivers.status(DriverName::Config),
        &DriverStatus::Dead(DriverError::Panicked)
    );
    assert_eq!(
        model.workspace.toasts.first().map(|toast| (
            toast.level,
            toast.title.as_str(),
            toast.text.as_deref()
        )),
        Some((
            ToastLevel::Error,
            "The config driver stopped",
            Some("panicked")
        ))
    );
}

#[test]
fn a_driver_death_while_stopped_is_refused() {
    let mut model = playing_model(3);
    model.drivers.record_mut(DriverName::Audio).status = DriverStatus::Stopped;
    let before = model.drivers.clone();

    let answer = update(&mut model, died(DriverName::Audio), Moment::default());

    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model.drivers, before);
}

struct StrategyRow {
    driver_name: DriverName,
    prior_restarts: usize,
    expected_status: fn() -> DriverStatus,
    check: fn(&Cmd),
}

fn starts_with_restart_and_starts_audio(cmd: &Cmd) {
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(matches!(
        effects.first(),
        Some(Effect::Restart(DriverName::Audio))
    ));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::ListDevices)))
    );
}

fn degrades_audio_with_a_toast(cmd: &Cmd) {
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Animate(Cue::ToastRaised)))
    );
}

fn changes_nothing(cmd: &Cmd) {
    assert_eq!(cmd, &Cmd::none());
}

fn restarts_and_rescans_library(cmd: &Cmd) {
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(matches!(
        effects.first(),
        Some(Effect::Restart(DriverName::Library))
    ));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Library(LibraryCmd::Scan { .. })))
    );
}

#[rstest::rstest]
#[case::restart_emits_restart_and_startup(StrategyRow {
    driver_name: DriverName::Audio,
    prior_restarts: 0,
    expected_status: || DriverStatus::Running,
    check: starts_with_restart_and_starts_audio,
})]
#[case::restart_budget_spent_degrades_with_a_toast(StrategyRow {
    driver_name: DriverName::Audio,
    prior_restarts: 3,
    expected_status: || DriverStatus::Dead(DriverError::Panicked),
    check: degrades_audio_with_a_toast,
})]
#[case::degrade_silent_changes_nothing_but_status(StrategyRow {
    driver_name: DriverName::Macos,
    prior_restarts: 0,
    expected_status: || DriverStatus::Dead(DriverError::Panicked),
    check: changes_nothing,
})]
#[case::library_restart_rescans(StrategyRow {
    driver_name: DriverName::Library,
    prior_restarts: 0,
    expected_status: || DriverStatus::Running,
    check: restarts_and_rescans_library,
})]
fn a_death_follows_the_supervision(#[case] row: StrategyRow) {
    let mut model = Model::default();
    let now = Moment::new(Duration::from_secs(100));
    for _ in 0..row.prior_restarts {
        model
            .drivers
            .record_mut(row.driver_name)
            .restarts
            .record(Moment::new(Duration::from_secs(90)));
    }

    let cmd = update(&mut model, died(row.driver_name), now).unwrap();

    assert_eq!(
        model.drivers.status(row.driver_name),
        &(row.expected_status)()
    );
    (row.check)(&cmd);
}

#[test]
fn congestion_raises_one_toast_naming_the_driver() {
    let mut model = Model::default();

    let cmd = update(
        &mut model,
        Message::Driver {
            driver_name: DriverName::Library,
            event: DriverEvent::Full,
        },
        Moment::default(),
    )
    .unwrap();

    assert_eq!(
        cmd,
        Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()])
    );
    assert_eq!(
        model
            .workspace
            .toasts
            .first()
            .map(|toast| (toast.level, toast.title.as_str())),
        Some((ToastLevel::Info, "The library driver is falling behind"))
    );
    assert_eq!(
        model.drivers.status(DriverName::Library),
        &DriverStatus::Running
    );
}

struct ResumeRow {
    player: fn() -> Player,
    position: Duration,
    playback: Playback,
}

fn playing_at(position: Duration) -> Player {
    Player::Playing {
        track: dated_track(0),
        playhead: Playhead::anchored(position, Moment::default(), Speed::default()),
        preloaded: None,
    }
}

fn paused_at(position: Duration) -> Player {
    Player::Paused {
        track: bare_track(0),
        position,
        by: PausedBy::Listener,
    }
}

#[rstest::rstest]
#[case::playing(ResumeRow {
    player: || playing_at(Duration::from_secs(30)),
    position: Duration::from_secs(30),
    playback: Playback::Playing,
})]
#[case::paused(ResumeRow {
    player: || paused_at(Duration::from_secs(45)),
    position: Duration::from_secs(45),
    playback: Playback::Paused,
})]
fn an_audio_restart_resumes_from_the_same_place(#[case] row: ResumeRow) {
    let mut model = Model {
        player: (row.player)(),
        ..Model::default()
    };
    let path: std::path::PathBuf = model
        .player
        .current()
        .and_then(|track| track.local_path())
        .unwrap()
        .to_path_buf();

    let cmd = update(&mut model, died(DriverName::Audio), Moment::default()).unwrap();
    let effects: Vec<&Effect> = cmd.effects().collect();

    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Load(TrackLoad { path: loaded, .. })) if loaded == &path
    )));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Seek(seek)) if *seek == row.position
    )));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::SetPlayback(playback)) if *playback == row.playback
    )));
    assert_eq!(
        model.drivers.status(DriverName::Audio),
        &DriverStatus::Running
    );
}
