use std::time::Duration;

use kernel::{
    AudioCmd,
    Cmd,
    Cue,
    DriverMessage,
    Effect,
    LibraryCmd,
    Message,
    Model,
    Moment,
    Pause,
    Playback,
    Player,
    Playhead,
    Preload,
    Speed,
    Timer,
    ToastLevel,
    domain::{Announce, Driver, DriverError, DriverStatus, Fallback, Supervision},
    update::update,
};

use crate::support::{bare_track, dated_track, first_toast_expiry, playing_model};

fn died(driver: Driver) -> Message {
    Message::Driver(
        driver,
        DriverMessage::Died(DriverError::Panicked("index out of bounds".to_string())),
    )
}

#[test]
fn a_driver_death_is_recorded_and_told_as_an_error() {
    let mut model = playing_model(3);

    let cmd = update(&mut model, died(Driver::Config), Moment::default()).unwrap();

    assert_eq!(
        cmd,
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            first_toast_expiry()
        ])
    );

    assert_eq!(
        model.drivers.status(Driver::Config),
        &DriverStatus::Dead(DriverError::Panicked("index out of bounds".to_string()))
    );
    assert_eq!(
        model
            .workspace
            .toast
            .as_ref()
            .map(|toast| (toast.level, toast.text.as_str())),
        Some((
            ToastLevel::Error,
            "The config driver stopped: panicked: index out of bounds"
        ))
    );
}

struct StrategyRow {
    driver: Driver,
    strategy: Option<Supervision>,
    prior_restarts: usize,
    expected_status: fn() -> DriverStatus,
    check: fn(&Cmd),
}

fn starts_with_restart_and_starts_audio(cmd: &Cmd) {
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(matches!(
        effects.first(),
        Some(Effect::Restart(Driver::Audio))
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

fn schedules_a_one_second_restart(cmd: &Cmd) {
    assert_eq!(
        cmd,
        &Cmd::One(Effect::After {
            delay: Duration::from_secs(1),
            message: Timer::Restart(Driver::Audio),
        })
    );
}

fn changes_nothing(cmd: &Cmd) {
    assert_eq!(cmd, &Cmd::None);
}

fn quits(cmd: &Cmd) {
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Quit)));
}

fn restarts_and_rescans_library(cmd: &Cmd) {
    let effects: Vec<&Effect> = cmd.effects().collect();
    assert!(matches!(
        effects.first(),
        Some(Effect::Restart(Driver::Library))
    ));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Library(LibraryCmd::Scan { .. })))
    );
}

#[rstest::rstest]
#[case::restart_emits_restart_and_boot(StrategyRow {
    driver: Driver::Audio,
    strategy: None,
    prior_restarts: 0,
    expected_status: || DriverStatus::Running,
    check: starts_with_restart_and_starts_audio,
})]
#[case::restart_budget_spent_degrades_with_a_toast(StrategyRow {
    driver: Driver::Audio,
    strategy: None,
    prior_restarts: 3,
    expected_status: || DriverStatus::Dead(DriverError::Panicked(
        "index out of bounds".to_string()
    )),
    check: degrades_audio_with_a_toast,
})]
#[case::backoff_schedules_a_restart_timer(StrategyRow {
    driver: Driver::Audio,
    strategy: Some(Supervision::Backoff {
        attempts: 3,
        first: Duration::from_secs(1),
        longest: Duration::from_secs(8),
        then: Fallback::Degrade(Announce::Toast),
    }),
    prior_restarts: 0,
    expected_status: || DriverStatus::Dead(DriverError::Panicked(
        "index out of bounds".to_string()
    )),
    check: schedules_a_one_second_restart,
})]
#[case::degrade_silent_changes_nothing_but_status(StrategyRow {
    driver: Driver::Macos,
    strategy: None,
    prior_restarts: 0,
    expected_status: || DriverStatus::Dead(DriverError::Panicked(
        "index out of bounds".to_string()
    )),
    check: changes_nothing,
})]
#[case::fatal_quits(StrategyRow {
    driver: Driver::Config,
    strategy: Some(Supervision::Fallback(Fallback::Quit)),
    prior_restarts: 0,
    expected_status: || DriverStatus::Dead(DriverError::Panicked(
        "index out of bounds".to_string()
    )),
    check: quits,
})]
#[case::library_restart_rescans(StrategyRow {
    driver: Driver::Library,
    strategy: None,
    prior_restarts: 0,
    expected_status: || DriverStatus::Running,
    check: restarts_and_rescans_library,
})]
fn a_death_follows_the_strategy(#[case] row: StrategyRow) {
    let mut model = Model::default();
    if let Some(strategy) = row.strategy {
        model.drivers = model.drivers.with_strategy(row.driver, strategy);
    }
    let now = Moment::new(Duration::from_secs(100));
    for _ in 0..row.prior_restarts {
        model
            .drivers
            .record_mut(row.driver)
            .restarts
            .record(Moment::new(Duration::from_secs(90)));
    }

    let cmd = update(&mut model, died(row.driver), now).unwrap();

    assert_eq!(model.drivers.status(row.driver), &(row.expected_status)());
    (row.check)(&cmd);
}

struct TimerRow {
    start: DriverStatus,
    expected_status: DriverStatus,
    landed: fn(&Cmd) -> bool,
}

#[rstest::rstest]
#[case::dead_restarts(TimerRow {
    start: DriverStatus::Dead(DriverError::Panicked("boom".to_string())),
    expected_status: DriverStatus::Running,
    landed: |cmd| cmd
        .effects()
        .next()
        .is_some_and(|effect| matches!(effect, Effect::Restart(Driver::Config))),
})]
#[case::stopped_ignores(TimerRow {
    start: DriverStatus::Stopped,
    expected_status: DriverStatus::Stopped,
    landed: |cmd| cmd == &Cmd::None,
})]
#[case::running_ignores(TimerRow {
    start: DriverStatus::Running,
    expected_status: DriverStatus::Running,
    landed: |cmd| cmd == &Cmd::None,
})]
fn a_restart_timer_lands(#[case] row: TimerRow) {
    let mut model = Model::default();
    model.drivers.record_mut(Driver::Config).status = row.start;

    let cmd = update(
        &mut model,
        Message::Elapsed(Timer::Restart(Driver::Config)),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(model.drivers.status(Driver::Config), &row.expected_status);
    assert!((row.landed)(&cmd));
}

#[test]
fn congestion_raises_one_toast_naming_the_driver() {
    let mut model = Model::default();

    let cmd = update(
        &mut model,
        Message::Driver(Driver::Library, DriverMessage::Full),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(
        cmd,
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            first_toast_expiry()
        ])
    );
    assert_eq!(
        model
            .workspace
            .toast
            .as_ref()
            .map(|toast| (toast.level, toast.text.as_str())),
        Some((ToastLevel::Info, "The library driver is falling behind"))
    );
    assert_eq!(
        model.drivers.status(Driver::Library),
        &DriverStatus::Running
    );
}

struct ResumeRow {
    player: fn() -> Player,
    at: Duration,
    playback: Playback,
}

fn playing_at(at: Duration) -> Player {
    Player::Playing {
        track: dated_track(0),
        head: Playhead::anchored(at, Moment::default(), Speed::default()),
        preload: Preload::None,
    }
}

fn paused_at(at: Duration) -> Player {
    Player::Paused {
        track: bare_track(0),
        at,
        pause: Pause::ByListener,
    }
}

#[rstest::rstest]
#[case::playing(ResumeRow {
    player: || playing_at(Duration::from_secs(30)),
    at: Duration::from_secs(30),
    playback: Playback::Playing,
})]
#[case::paused(ResumeRow {
    player: || paused_at(Duration::from_secs(45)),
    at: Duration::from_secs(45),
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
        .map(|track| track.path().to_path_buf())
        .unwrap();

    let cmd = update(&mut model, died(Driver::Audio), Moment::default()).unwrap();
    let effects: Vec<&Effect> = cmd.effects().collect();

    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Load { path: loaded, .. }) if loaded == &path
    )));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Seek(seek)) if *seek == row.at
    )));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Playback(playback)) if *playback == row.playback
    )));
    assert_eq!(model.drivers.status(Driver::Audio), &DriverStatus::Running);
}
