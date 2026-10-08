use std::{
    env,
    time::{Duration, Instant},
};

use kernel::{
    cmd::{Cmds, RemoteCmd},
    domain::{
        favorites::Favorite,
        io_error::IoError,
        server::{
            ApiCode,
            PlayReport,
            RemoteError,
            Scrobble,
            ServerName,
            ServerTrackId,
        },
        time::Moment,
    },
    message::RemoteEvent,
    update::machine::{Driver, LoopEffect, Machine, Unhandled},
};
use remote::{
    driver::{REPORT_RETRY, RemoteDriver, RemoteEffect},
    job::{RemoteJob, SignedReport},
    message::{RemoteMessage, RemoteTimer},
};

use crate::unit::driver::{
    Answer,
    answered,
    jobs,
    session,
    star_cmd,
    star_job,
    stored,
    waits,
};

fn play_report(scrobble: Scrobble) -> PlayReport {
    PlayReport {
        server_name: ServerName::new("a"),
        server_track_id: ServerTrackId::new("c41d"),
        scrobble,
    }
}

fn report_cmd(name: &str, scrobble: Scrobble) -> Option<RemoteCmd> {
    Some(RemoteCmd::Report {
        session: session()?,
        play_report: PlayReport {
            server_name: ServerName::new(name),
            ..play_report(scrobble)
        },
    })
}

fn report_job(name: &str, scrobbles: &[Scrobble]) -> Vec<RemoteJob> {
    session()
        .map(|session| {
            RemoteJob::Report(
                scrobbles
                    .iter()
                    .map(|scrobble| SignedReport {
                        session: session.clone(),
                        play_report: PlayReport {
                            server_name: ServerName::new(name),
                            ..play_report(*scrobble)
                        },
                    })
                    .collect(),
            )
        })
        .into_iter()
        .collect()
}

fn reported(
    name: &str,
    scrobbles: &[Scrobble],
    result: Result<(), RemoteError>,
) -> RemoteMessage {
    RemoteMessage::Reported {
        play_reports: scrobbles
            .iter()
            .map(|scrobble| PlayReport {
                server_name: ServerName::new(name),
                ..play_report(*scrobble)
            })
            .collect(),
        result,
    }
}

fn forget_cmd(name: &str) -> Option<RemoteCmd> {
    stored(name).map(|connection| RemoteCmd::Forget(connection.account))
}

fn ordered(remote_cmds: [Option<RemoteCmd>; 2]) -> RemoteMessage {
    RemoteMessage::Cmds(Cmds {
        cmds: remote_cmds.into_iter().flatten().collect(),
        at: Instant::now(),
    })
}

fn played() -> Scrobble {
    Scrobble::Played(Moment::new(Duration::from_millis(1_700_000_000_123)))
}

#[test]
fn a_report_runs_beside_a_star() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let asked = jobs(answered(
        &mut driver,
        ordered([
            star_cmd("c41d", Favorite::Yes),
            report_cmd("a", Scrobble::NowPlaying),
        ]),
    ));

    assert_eq!(
        asked,
        Some((
            star_job("c41d", Favorite::Yes)
                .into_iter()
                .chain(report_job("a", &[Scrobble::NowPlaying]))
                .collect(),
            vec![]
        ))
    );
}

#[test]
fn an_offline_server_keeps_its_reports_and_the_retry_sends_them_with_the_new_ones() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };

    let asked = jobs(answered(
        &mut driver,
        ordered([
            report_cmd("a", Scrobble::NowPlaying),
            report_cmd("a", played()),
        ]),
    ));
    let failed = answered(
        &mut driver,
        reported("a", &[Scrobble::NowPlaying], Err(remote_error.clone())),
    );
    let waiting = jobs(answered(
        &mut driver,
        ordered([report_cmd("a", Scrobble::NowPlaying), None]),
    ));
    let retried = jobs(answered(
        &mut driver,
        RemoteMessage::Elapsed(RemoteTimer::Retry),
    ));
    let accepted = jobs(answered(
        &mut driver,
        reported("a", &[played(), Scrobble::NowPlaying], Ok(())),
    ));

    assert_eq!(
        asked,
        Some((report_job("a", &[Scrobble::NowPlaying, played()]), vec![]))
    );
    assert_eq!(
        waits(failed),
        Some((
            vec![(REPORT_RETRY, RemoteTimer::Retry)],
            vec![RemoteEvent::Error(remote_error)]
        ))
    );
    assert_eq!(waiting, Some((vec![], vec![])));
    assert_eq!(
        retried,
        Some((report_job("a", &[played(), Scrobble::NowPlaying]), vec![]))
    );
    assert_eq!(accepted, Some((vec![], vec![])));
    assert_eq!(
        driver
            .transition(RemoteMessage::Elapsed(RemoteTimer::Retry))
            .err(),
        Some(Unhandled)
    );
    assert_eq!(
        driver.transition(reported("a", &[], Ok(()))).err(),
        Some(Unhandled)
    );
}

#[test]
fn a_failed_server_lets_the_next_server_report_first_on_the_retry() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };

    let asked = jobs(answered(
        &mut driver,
        ordered([report_cmd("a", played()), report_cmd("b", played())]),
    ));
    let failed = jobs(answered(
        &mut driver,
        reported("a", &[], Err(remote_error.clone())),
    ));
    let retried = jobs(answered(
        &mut driver,
        RemoteMessage::Elapsed(RemoteTimer::Retry),
    ));
    let next = jobs(answered(&mut driver, reported("b", &[played()], Ok(()))));

    assert_eq!(asked, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(
        failed,
        Some((vec![], vec![RemoteEvent::Error(remote_error)]))
    );
    assert_eq!(retried, Some((report_job("b", &[played()]), vec![])));
    assert_eq!(next, Some((report_job("a", &[played()]), vec![])));
}

fn executed(answer: Option<Answer>) -> Option<Vec<RemoteEffect>> {
    answer.map(|(effects, _events)| {
        effects
            .into_iter()
            .filter_map(|effect| {
                if let LoopEffect::Execute(remote_effect) = effect {
                    Some(remote_effect)
                } else {
                    None
                }
            })
            .collect()
    })
}

#[test]
fn the_flush_file_round_trips_the_pending_reports_without_their_sessions() {
    let reports_path =
        env::temp_dir().join(format!("sifr-reports-{}.json", std::process::id()));
    let mut driver = RemoteDriver::new(env::temp_dir(), reports_path.clone());
    let play_reports: Vec<PlayReport> = [Scrobble::NowPlaying, played()]
        .into_iter()
        .map(play_report)
        .collect();

    let started = answered(&mut driver, RemoteMessage::Started);
    let flushed = answered(
        &mut driver,
        ordered([
            report_cmd("a", Scrobble::NowPlaying),
            Some(RemoteCmd::Flush(vec![play_report(played())])),
        ]),
    );
    let written = driver.execute(RemoteEffect::Flush(play_reports));
    let restored = driver.execute(RemoteEffect::Restore);
    let event = restored.and_then(|message| answered(&mut driver, message));
    let removed = std::fs::remove_file(&reports_path);
    let missing = driver.execute(RemoteEffect::Restore);
    let empty = answered(&mut driver, RemoteMessage::Restored(Ok(Vec::new())));

    assert_eq!(executed(started), Some(vec![RemoteEffect::Restore]));
    assert_eq!(
        executed(flushed),
        Some(vec![RemoteEffect::Flush(vec![
            play_report(played()),
            play_report(Scrobble::NowPlaying)
        ])])
    );
    assert!(matches!(written, Some(RemoteMessage::Saved(Ok(())))));
    assert_eq!(
        event.map(|(_effects, events)| events),
        Some(vec![RemoteEvent::Restored(Ok(vec![play_report(played())]))])
    );
    assert!(removed.is_ok());
    assert!(
        matches!(missing, Some(RemoteMessage::Restored(Ok(restored_play_reports))) if restored_play_reports.is_empty())
    );
    assert_eq!(
        empty.map(|(effects, events)| (effects.len(), events)),
        Some((0, vec![]))
    );
}

#[test]
fn an_unreadable_reports_file_is_an_error_not_an_empty_list() {
    let reports_path =
        env::temp_dir().join(format!("sifr-reports-bad-{}.json", std::process::id()));
    let written = std::fs::write(&reports_path, b"not json");
    let mut driver = RemoteDriver::new(env::temp_dir(), reports_path.clone());

    let restored = driver.execute(RemoteEffect::Restore);
    let event = restored.and_then(|message| answered(&mut driver, message));
    let removed = std::fs::remove_file(&reports_path);

    assert!(written.is_ok());
    assert_eq!(
        event.map(|(_effects, events)| events),
        Some(vec![RemoteEvent::Restored(Err(IoError::Malformed))])
    );
    assert!(removed.is_ok());
}

#[test]
fn a_failed_write_of_no_reports_answers_unsaved_without_a_server() {
    let blocker =
        env::temp_dir().join(format!("sifr-reports-file-{}", std::process::id()));
    let written = std::fs::write(&blocker, b"");
    let mut driver = RemoteDriver::new(env::temp_dir(), blocker.join("reports.json"));

    let saved = driver.execute(RemoteEffect::Flush(Vec::new()));
    let event = saved.and_then(|message| answered(&mut driver, message));
    let removed = std::fs::remove_file(&blocker);

    assert!(written.is_ok());
    assert!(matches!(
        event.map(|(_effects, events)| events).as_deref(),
        Some([RemoteEvent::Unsaved(_io_error)])
    ));
    assert!(removed.is_ok());
}

#[test]
fn a_refused_report_is_dropped_once_and_the_rest_go_on() {
    let remote_error = RemoteError::Api {
        server_name: ServerName::new("a"),
        api_code: ApiCode(70),
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let asked = jobs(answered(
        &mut driver,
        ordered([
            report_cmd("a", Scrobble::NowPlaying),
            report_cmd("a", played()),
        ]),
    ));
    let refused = jobs(answered(
        &mut driver,
        reported("a", &[], Err(remote_error.clone())),
    ));
    let accepted = jobs(answered(&mut driver, reported("a", &[played()], Ok(()))));

    assert_eq!(
        asked,
        Some((report_job("a", &[Scrobble::NowPlaying, played()]), vec![]))
    );
    assert_eq!(
        refused,
        Some((
            report_job("a", &[played()]),
            vec![RemoteEvent::Error(remote_error)]
        ))
    );
    assert_eq!(accepted, Some((vec![], vec![])));
    assert_eq!(
        driver
            .transition(RemoteMessage::Elapsed(RemoteTimer::Retry))
            .err(),
        Some(Unhandled)
    );
}

#[test]
fn a_streak_of_failed_retries_answers_one_error_until_a_success_ends_it() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = RemoteError::Unreachable {
        server_name: ServerName::new("a"),
        source: IoError::Other,
    };
    let retry = || RemoteMessage::Elapsed(RemoteTimer::Retry);

    let asked = jobs(answered(
        &mut driver,
        ordered([report_cmd("a", played()), None]),
    ));
    let failures = [
        answered(&mut driver, reported("a", &[], Err(remote_error.clone()))),
        answered(&mut driver, retry()).and(answered(
            &mut driver,
            reported("a", &[], Err(remote_error.clone())),
        )),
        answered(&mut driver, retry()).and(answered(
            &mut driver,
            reported("a", &[], Err(remote_error.clone())),
        )),
    ]
    .map(waits);
    let retried = jobs(answered(&mut driver, retry()));
    let accepted = jobs(answered(&mut driver, reported("a", &[played()], Ok(()))));
    let asked_again = jobs(answered(
        &mut driver,
        ordered([report_cmd("a", played()), None]),
    ));
    let failed_again = waits(answered(
        &mut driver,
        reported("a", &[], Err(remote_error.clone())),
    ));

    assert_eq!(asked, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(
        failures,
        [
            Some((
                vec![(REPORT_RETRY, RemoteTimer::Retry)],
                vec![RemoteEvent::Error(remote_error.clone())]
            )),
            Some((vec![(REPORT_RETRY, RemoteTimer::Retry)], vec![])),
            Some((vec![(REPORT_RETRY, RemoteTimer::Retry)], vec![])),
        ]
    );
    assert_eq!(retried, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(accepted, Some((vec![], vec![])));
    assert_eq!(asked_again, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(
        failed_again,
        Some((
            vec![(REPORT_RETRY, RemoteTimer::Retry)],
            vec![RemoteEvent::Error(remote_error)]
        ))
    );
}

#[test]
fn a_failed_now_playing_is_neither_retried_nor_written() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };

    let asked = jobs(answered(
        &mut driver,
        ordered([
            report_cmd("a", Scrobble::NowPlaying),
            report_cmd("a", played()),
        ]),
    ));
    let failed = jobs(answered(
        &mut driver,
        reported("a", &[], Err(remote_error.clone())),
    ));
    let retried = jobs(answered(
        &mut driver,
        RemoteMessage::Elapsed(RemoteTimer::Retry),
    ));
    let flushed = answered(
        &mut driver,
        ordered([
            Some(RemoteCmd::Flush(vec![play_report(Scrobble::NowPlaying)])),
            None,
        ]),
    );

    assert_eq!(
        asked,
        Some((report_job("a", &[Scrobble::NowPlaying, played()]), vec![]))
    );
    assert_eq!(
        failed,
        Some((vec![], vec![RemoteEvent::Error(remote_error)]))
    );
    assert_eq!(retried, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(
        executed(flushed),
        Some(vec![RemoteEffect::Flush(vec![
            play_report(Scrobble::NowPlaying),
            play_report(played())
        ])])
    );
}

#[test]
fn a_forget_drops_the_server_reports_from_the_queue_and_the_file() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let asked = jobs(answered(
        &mut driver,
        ordered([report_cmd("a", played()), report_cmd("home", played())]),
    ));
    let forgotten = answered(&mut driver, ordered([forget_cmd("home"), None]));
    let accepted = jobs(answered(&mut driver, reported("a", &[played()], Ok(()))));

    assert_eq!(asked, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(
        executed(forgotten),
        Some(vec![RemoteEffect::Flush(vec![play_report(played())])])
    );
    assert_eq!(accepted, Some((vec![], vec![])));
}

#[test]
fn a_sent_report_leaves_the_written_file_at_once_and_a_restart_restores_the_rest() {
    let reports_path =
        env::temp_dir().join(format!("sifr-reports-sent-{}.json", std::process::id()));
    let mut driver = RemoteDriver::new(env::temp_dir(), reports_path.clone());
    let later_scrobble =
        Scrobble::Played(Moment::new(Duration::from_millis(1_700_000_100_000)));
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };

    let asked = answered(
        &mut driver,
        ordered([report_cmd("a", played()), report_cmd("a", later_scrobble)]),
    );
    let failed = answered(&mut driver, reported("a", &[played()], Err(remote_error)));
    let written_messages: Vec<RemoteMessage> = executed(failed)
        .into_iter()
        .flatten()
        .filter_map(|remote_effect| driver.execute(remote_effect))
        .collect();
    let mut restarted_driver = RemoteDriver::new(env::temp_dir(), reports_path.clone());
    let kept = restarted_driver
        .execute(RemoteEffect::Restore)
        .and_then(|message| answered(&mut restarted_driver, message));
    let forgotten = answered(&mut restarted_driver, ordered([forget_cmd("a"), None]));
    let removed = std::fs::remove_file(&reports_path);

    assert_eq!(
        executed(asked),
        Some(vec![
            RemoteEffect::Flush(vec![play_report(played())]),
            RemoteEffect::Flush(vec![
                play_report(played()),
                play_report(later_scrobble)
            ]),
        ])
    );
    assert!(matches!(
        written_messages.as_slice(),
        [RemoteMessage::Saved(Ok(()))]
    ));
    assert_eq!(
        kept.map(|(_effects, events)| events),
        Some(vec![RemoteEvent::Restored(Ok(vec![play_report(
            later_scrobble
        )]))])
    );
    assert_eq!(executed(forgotten), Some(vec![RemoteEffect::Flush(vec![])]));
    assert!(removed.is_ok());
}

#[test]
fn a_restored_report_ordered_again_is_written_once() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let kept = answered(
        &mut driver,
        RemoteMessage::Restored(Ok(vec![play_report(played())])),
    );
    let asked = answered(&mut driver, ordered([report_cmd("a", played()), None]));

    assert_eq!(
        kept.map(|(_effects, events)| events),
        Some(vec![RemoteEvent::Restored(Ok(vec![play_report(played())]))])
    );
    assert_eq!(
        executed(asked),
        Some(vec![RemoteEffect::Flush(vec![play_report(played())])])
    );
}

#[test]
fn two_servers_that_fail_by_turns_answer_one_error_each() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = |name: &str| RemoteError::Unreachable {
        server_name: ServerName::new(name),
        source: IoError::Other,
    };
    let retry = || RemoteMessage::Elapsed(RemoteTimer::Retry);

    let asked = jobs(answered(
        &mut driver,
        ordered([report_cmd("a", played()), report_cmd("b", played())]),
    ));
    let failures = [
        answered(&mut driver, reported("a", &[], Err(remote_error("a")))),
        answered(&mut driver, retry()).and(answered(
            &mut driver,
            reported("b", &[], Err(remote_error("b"))),
        )),
        answered(&mut driver, retry()).and(answered(
            &mut driver,
            reported("a", &[], Err(remote_error("a"))),
        )),
        answered(&mut driver, retry()).and(answered(
            &mut driver,
            reported("b", &[], Err(remote_error("b"))),
        )),
    ]
    .map(waits);

    assert_eq!(asked, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(
        failures,
        [
            Some((
                vec![(REPORT_RETRY, RemoteTimer::Retry)],
                vec![RemoteEvent::Error(remote_error("a"))]
            )),
            Some((
                vec![(REPORT_RETRY, RemoteTimer::Retry)],
                vec![RemoteEvent::Error(remote_error("b"))]
            )),
            Some((vec![(REPORT_RETRY, RemoteTimer::Retry)], vec![])),
            Some((vec![(REPORT_RETRY, RemoteTimer::Retry)], vec![])),
        ]
    );
}

#[test]
fn a_connect_success_ends_the_streak_so_the_next_failure_answers_an_error_again() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = RemoteError::Unreachable {
        server_name: ServerName::new("a"),
        source: IoError::Other,
    };

    let failed = waits(
        answered(&mut driver, ordered([report_cmd("a", played()), None])).and(
            answered(&mut driver, reported("a", &[], Err(remote_error.clone()))),
        ),
    );
    let connecting = jobs(answered(
        &mut driver,
        ordered([stored("a").map(RemoteCmd::Connect), None]),
    ));
    let online = session()
        .and_then(|session| {
            answered(
                &mut driver,
                RemoteMessage::Connected {
                    server_name: ServerName::new("a"),
                    result: Ok(session),
                    stored: Ok(()),
                },
            )
        })
        .map(|(_effects, events)| events);
    let retried = jobs(answered(
        &mut driver,
        RemoteMessage::Elapsed(RemoteTimer::Retry),
    ));
    let failed_again = waits(answered(
        &mut driver,
        reported("a", &[], Err(remote_error.clone())),
    ));

    let raised = Some((
        vec![(REPORT_RETRY, RemoteTimer::Retry)],
        vec![RemoteEvent::Error(remote_error)],
    ));
    assert_eq!(failed, raised);
    assert_eq!(
        connecting,
        stored("a").map(|connection| (vec![RemoteJob::Connect(connection)], vec![]))
    );
    assert_eq!(
        online,
        session().map(|session| vec![RemoteEvent::Connected {
            server_name: ServerName::new("a"),
            session,
        }])
    );
    assert_eq!(retried, Some((report_job("a", &[played()]), vec![])));
    assert_eq!(failed_again, raised);
}
