use std::{
    convert::Infallible,
    env,
    io::ErrorKind,
    time::{Duration, Instant},
};

use kernel::{
    cmd::{Cmds, RemoteCmd},
    domain::{
        io_error::IoError,
        revision::Revision,
        server::{
            Account,
            AlbumId,
            AlbumOrder,
            Connection,
            Credential,
            Endpoint,
            Listing,
            Page,
            RemoteError,
            Secret,
            ServerName,
            Session,
            UserName,
        },
    },
    message::RemoteEvent,
    update::machine::{LoopEffect, Machine, Unhandled},
};
use remote::{
    driver::{RemoteDriver, SEARCH_WAIT},
    job::RemoteJob,
    message::{RemoteMessage, RemoteTimer},
};

type Answer = (
    Vec<LoopEffect<Infallible, RemoteJob, RemoteMessage>>,
    Vec<RemoteEvent>,
);

fn connection(name: &str, credential: Credential) -> Option<Connection> {
    Some(Connection {
        account: Account {
            server_name: ServerName::new(name),
            endpoint: Endpoint::parse("https://music.example").ok()?,
            user_name: UserName::new("alice").ok()?,
        },
        credential,
    })
}

fn stored(name: &str) -> Option<Connection> {
    connection(name, Credential::Stored)
}

fn typed(name: &str, password: &str) -> Option<Connection> {
    connection(name, Credential::Typed(Secret::new(password).ok()?))
}

fn asked(connections: Vec<Connection>) -> RemoteMessage {
    RemoteMessage::Cmds(Cmds {
        cmds: connections.into_iter().map(RemoteCmd::Connect).collect(),
        at: Instant::now(),
    })
}

fn session() -> Option<Session> {
    Some(Session::new(
        Endpoint::parse("https://music.example").ok()?,
        "u=alice",
    ))
}

fn connected(name: &str, stored: Result<(), RemoteError>) -> Option<RemoteMessage> {
    Some(RemoteMessage::Connected {
        server_name: ServerName::new(name),
        result: Ok(session()?),
        stored,
    })
}

fn answered(driver: &mut RemoteDriver, message: RemoteMessage) -> Option<Answer> {
    driver
        .transition(message)
        .ok()
        .map(kernel::cmd::Cmd::into_parts)
}

fn jobs(answer: Option<Answer>) -> Option<(Vec<RemoteJob>, Vec<RemoteEvent>)> {
    answer.map(|(effects, events)| {
        let jobs = effects
            .into_iter()
            .filter_map(|effect| {
                if let LoopEffect::Run(job) = effect {
                    Some(job)
                } else {
                    None
                }
            })
            .collect();
        (jobs, events)
    })
}

fn online(name: &str) -> Option<RemoteEvent> {
    Some(RemoteEvent::Connected {
        server_name: ServerName::new(name),
        session: session()?,
    })
}

#[test]
fn two_connects_run_the_first_and_the_second_waits_for_its_answer() {
    let (Some(a), Some(b)) = (stored("a"), stored("b")) else {
        panic!("valid connections");
    };
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert_eq!(
        jobs(answered(&mut driver, asked(vec![a.clone(), b.clone()]))),
        Some((vec![RemoteJob::Connect(a)], vec![]))
    );
    assert_eq!(
        jobs(connected("a", Ok(())).and_then(|message| answered(&mut driver, message))),
        Some((
            vec![RemoteJob::Connect(b)],
            online("a").into_iter().collect()
        ))
    );
    assert_eq!(
        jobs(connected("b", Ok(())).and_then(|message| answered(&mut driver, message))),
        Some((vec![], online("b").into_iter().collect()))
    );
}

#[test]
fn a_connect_for_the_server_in_flight_is_queued_once_with_the_newest_credential() {
    let (Some(first), Some(second), Some(third)) =
        (stored("a"), typed("a", "old"), typed("a", "new"))
    else {
        panic!("valid connections");
    };
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert_eq!(
        jobs(answered(&mut driver, asked(vec![first.clone()]))),
        Some((vec![RemoteJob::Connect(first)], vec![]))
    );
    assert_eq!(
        jobs(answered(&mut driver, asked(vec![second, third.clone()]))),
        Some((vec![], vec![]))
    );
    assert_eq!(
        jobs(connected("a", Ok(())).and_then(|message| answered(&mut driver, message))),
        Some((
            vec![RemoteJob::Connect(third)],
            online("a").into_iter().collect()
        ))
    );
    assert_eq!(
        jobs(connected("a", Ok(())).and_then(|message| answered(&mut driver, message))),
        Some((vec![], online("a").into_iter().collect()))
    );
}

#[test]
fn an_answer_for_a_server_not_in_flight_is_unhandled_and_changes_nothing() {
    let (Some(a), Some(b)) = (stored("a"), stored("b")) else {
        panic!("valid connections");
    };
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert!(driver.transition(asked(vec![a, b])).is_ok());
    let before = format!("{driver:?}");
    assert!(matches!(
        connected("b", Ok(())).map(|message| driver.transition(message)),
        Some(Err(Unhandled))
    ));
    assert_eq!(format!("{driver:?}"), before);
}

#[test]
fn an_empty_order_is_unhandled() {
    assert!(matches!(
        RemoteDriver::new(env::temp_dir()).transition(asked(vec![])),
        Err(Unhandled)
    ));
}

#[test]
fn a_refused_ping_reports_the_error() {
    let Some(a) = stored("a") else {
        panic!("valid connection");
    };
    let remote_error = RemoteError::NoPassword {
        server_name: ServerName::new("a"),
    };
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert!(driver.transition(asked(vec![a])).is_ok());
    assert_eq!(
        jobs(answered(
            &mut driver,
            RemoteMessage::Connected {
                server_name: ServerName::new("a"),
                result: Err(remote_error.clone()),
                stored: Ok(()),
            }
        )),
        Some((vec![], vec![RemoteEvent::Error(remote_error)]))
    );
}

#[test]
fn a_failed_store_answers_online_and_reports_the_keychain_error() {
    let Some(a) = typed("a", "hunter2") else {
        panic!("valid connection");
    };
    let remote_error = RemoteError::Keychain {
        server_name: ServerName::new("a"),
        source: IoError::from(ErrorKind::PermissionDenied),
    };
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert!(driver.transition(asked(vec![a])).is_ok());
    assert_eq!(
        jobs(
            connected("a", Err(remote_error.clone()))
                .and_then(|message| answered(&mut driver, message))
        ),
        Some((
            vec![],
            online("a")
                .into_iter()
                .chain([RemoteEvent::Error(remote_error)])
                .collect()
        ))
    );
}

fn list_cmd(name: &str, listing: Listing, revision: Revision) -> Option<RemoteMessage> {
    Some(RemoteMessage::Cmds(Cmds {
        cmds: vec![RemoteCmd::List {
            server_name: ServerName::new(name),
            session: session()?,
            listing,
            page: Page::default(),
            revision,
        }],
        at: Instant::now(),
    }))
}

fn list_job(name: &str, listing: Listing, revision: Revision) -> Option<RemoteJob> {
    Some(RemoteJob::List {
        server_name: ServerName::new(name),
        session: session()?,
        listing,
        page: Page::default(),
        revision,
    })
}

fn online_driver(name: &str) -> RemoteDriver {
    let mut driver = RemoteDriver::new(env::temp_dir());
    let Some(connection) = stored(name) else {
        return driver;
    };
    assert!(driver.transition(asked(vec![connection])).is_ok());
    assert!(
        connected(name, Ok(()))
            .is_some_and(|message| driver.transition(message).is_ok())
    );
    driver
}

#[test]
fn two_lists_give_one_in_flight_and_the_newest_waiting_and_the_answer_starts_the_waiting_one()
 {
    let first = Revision::default().next();
    let second = first.next();
    let third = second.next();
    let listing = Listing::Album(AlbumId::new("al-1"));
    let mut driver = online_driver("a");
    assert_eq!(
        jobs(
            list_cmd("a", Listing::Albums(AlbumOrder::Newest), first)
                .and_then(|message| answered(&mut driver, message))
        ),
        Some((
            list_job("a", Listing::Albums(AlbumOrder::Newest), first)
                .into_iter()
                .collect(),
            vec![]
        ))
    );
    assert_eq!(
        jobs(
            list_cmd("a", Listing::Albums(AlbumOrder::Random), second)
                .and_then(|message| answered(&mut driver, message))
        ),
        Some((vec![], vec![]))
    );
    assert_eq!(
        jobs(
            list_cmd("a", listing.clone(), third)
                .and_then(|message| answered(&mut driver, message))
        ),
        Some((vec![], vec![]))
    );
    assert_eq!(
        jobs(answered(
            &mut driver,
            RemoteMessage::Listed {
                server_name: ServerName::new("a"),
                listing: Listing::Albums(AlbumOrder::Newest),
                page: Page::default(),
                result: Ok(vec![]),
                revision: first,
            }
        )),
        Some((
            list_job("a", listing, third).into_iter().collect(),
            vec![RemoteEvent::Listed {
                server_name: ServerName::new("a"),
                listing: Listing::Albums(AlbumOrder::Newest),
                page: Page::default(),
                catalog_rows: vec![],
                revision: first,
            }]
        ))
    );
}

#[test]
fn a_listed_answer_that_is_not_in_flight_is_unhandled_and_changes_nothing() {
    let first = Revision::default().next();
    let mut driver = online_driver("a");
    assert!(
        list_cmd("a", Listing::Albums(AlbumOrder::Newest), first)
            .is_some_and(|message| driver.transition(message).is_ok())
    );
    let before = format!("{driver:?}");
    assert!(matches!(
        driver.transition(RemoteMessage::Listed {
            server_name: ServerName::new("a"),
            listing: Listing::Albums(AlbumOrder::Newest),
            page: Page::default(),
            result: Ok(vec![]),
            revision: first.next(),
        }),
        Err(Unhandled)
    ));
    assert_eq!(format!("{driver:?}"), before);
}

#[test]
fn a_list_with_no_connect_before_it_runs_from_the_orders_session() {
    let revision = Revision::default().next();
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert_eq!(
        jobs(
            list_cmd("a", Listing::Albums(AlbumOrder::Newest), revision)
                .and_then(|message| answered(&mut driver, message))
        ),
        Some((
            list_job("a", Listing::Albums(AlbumOrder::Newest), revision)
                .into_iter()
                .collect(),
            vec![]
        ))
    );
}

#[test]
fn a_failed_list_reports_the_error_and_frees_the_slot() {
    let first = Revision::default().next();
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };
    let mut driver = online_driver("a");
    assert!(
        list_cmd("a", Listing::Albums(AlbumOrder::Newest), first)
            .is_some_and(|message| driver.transition(message).is_ok())
    );
    assert_eq!(
        jobs(answered(
            &mut driver,
            RemoteMessage::Listed {
                server_name: ServerName::new("a"),
                listing: Listing::Albums(AlbumOrder::Newest),
                page: Page::default(),
                result: Err(remote_error.clone()),
                revision: first,
            }
        )),
        Some((vec![], vec![RemoteEvent::Error(remote_error)]))
    );
    assert_eq!(
        jobs(
            list_cmd("a", Listing::Albums(AlbumOrder::Random), first.next())
                .and_then(|message| answered(&mut driver, message))
        )
        .map(|(jobs, _events)| jobs.len()),
        Some(1)
    );
}

fn search_cmd(input: &str, revision: Revision) -> Option<RemoteMessage> {
    Some(RemoteMessage::Cmds(Cmds {
        cmds: vec![RemoteCmd::Search {
            server_name: ServerName::new("a"),
            session: session()?,
            input: input.to_owned(),
            revision,
        }],
        at: Instant::now(),
    }))
}

fn search_job(input: &str, revision: Revision) -> Option<RemoteJob> {
    Some(RemoteJob::Search {
        server_name: ServerName::new("a"),
        session: session()?,
        input: input.to_owned(),
        revision,
    })
}

type Waits = (Vec<(Duration, RemoteTimer)>, Vec<RemoteEvent>);

fn waits(answer: Option<Answer>) -> Option<Waits> {
    answer.map(|(effects, events)| {
        let waits = effects
            .into_iter()
            .filter_map(|effect| {
                if let LoopEffect::After {
                    delay,
                    message: RemoteMessage::Elapsed(remote_timer),
                } = effect
                {
                    Some((delay, remote_timer))
                } else {
                    None
                }
            })
            .collect();
        (waits, events)
    })
}

fn elapsed(revision: Revision) -> RemoteMessage {
    RemoteMessage::Elapsed(RemoteTimer::Search(revision))
}

#[test]
fn three_searches_within_the_wait_give_one_job_with_the_last_input() {
    let first = Revision::default().next();
    let second = first.next();
    let third = second.next();
    let mut driver = RemoteDriver::new(env::temp_dir());
    for (input, revision) in [("m", first), ("mi", second), ("mil", third)] {
        assert_eq!(
            search_cmd(input, revision)
                .and_then(|message| waits(answered(&mut driver, message))),
            Some((vec![(SEARCH_WAIT, RemoteTimer::Search(revision))], vec![]))
        );
    }
    for revision in [first, second] {
        assert!(matches!(
            driver.transition(elapsed(revision)),
            Err(Unhandled)
        ));
    }
    assert_eq!(
        jobs(answered(&mut driver, elapsed(third))),
        Some((search_job("mil", third).into_iter().collect(), vec![]))
    );
    assert!(matches!(driver.transition(elapsed(third)), Err(Unhandled)));
}

#[test]
fn an_empty_search_sends_no_job_answers_an_empty_found_and_drops_the_waiting_search() {
    let first = Revision::default().next();
    let second = first.next();
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert!(
        search_cmd("m", first)
            .is_some_and(|message| driver.transition(message).is_ok())
    );
    assert_eq!(
        search_cmd("", second)
            .and_then(|message| answered(&mut driver, message))
            .map(|(effects, events)| (effects.len(), events)),
        Some((
            0,
            vec![RemoteEvent::Found {
                server_name: ServerName::new("a"),
                catalog_rows: vec![],
                revision: second,
            }]
        ))
    );
    assert!(matches!(driver.transition(elapsed(first)), Err(Unhandled)));
}

#[test]
fn a_found_answer_gives_its_rows_and_a_failed_search_reports_the_error() {
    let revision = Revision::default().next();
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert_eq!(
        jobs(answered(
            &mut driver,
            RemoteMessage::Found {
                server_name: ServerName::new("a"),
                result: Ok(vec![]),
                revision,
            }
        )),
        Some((
            vec![],
            vec![RemoteEvent::Found {
                server_name: ServerName::new("a"),
                catalog_rows: vec![],
                revision,
            }]
        ))
    );
    assert_eq!(
        jobs(answered(
            &mut driver,
            RemoteMessage::Found {
                server_name: ServerName::new("a"),
                result: Err(remote_error.clone()),
                revision,
            }
        )),
        Some((vec![], vec![RemoteEvent::Error(remote_error)]))
    );
}

#[test]
fn a_search_with_no_connect_before_it_runs_from_the_orders_session() {
    let revision = Revision::default().next();
    let mut driver = RemoteDriver::new(env::temp_dir());
    assert!(
        search_cmd("mil", revision)
            .is_some_and(|message| driver.transition(message).is_ok())
    );
    assert_eq!(
        jobs(answered(&mut driver, elapsed(revision))),
        Some((search_job("mil", revision).into_iter().collect(), vec![]))
    );
}
