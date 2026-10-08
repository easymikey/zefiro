use std::{
    env,
    io::ErrorKind,
    sync::Arc,
    time::{Duration, Instant},
};

use kernel::{
    cmd::{Cmd, Cmds, RemoteCmd},
    domain::{
        favorites::{Favorite, Favorites},
        io_error::IoError,
        revision::Revision,
        server::{
            Account,
            AlbumId,
            AlbumOrder,
            CacheKey,
            Connection,
            Credential,
            Endpoint,
            Listing,
            MediaFetch,
            Page,
            RemoteError,
            Secret,
            ServerName,
            ServerTrackId,
            Session,
            UserName,
        },
    },
    message::RemoteEvent,
    update::machine::{LoopEffect, Machine, Unhandled},
};
use remote::{
    driver::{RemoteDriver, RemoteEffect, SEARCH_WAIT},
    job::RemoteJob,
    message::{RemoteMessage, RemoteTimer},
};

pub(crate) type Answer = (
    Vec<LoopEffect<RemoteEffect, RemoteJob, RemoteMessage>>,
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

pub(crate) fn stored(name: &str) -> Option<Connection> {
    connection(name, Credential::Stored)
}

pub(crate) fn typed(name: &str, password: &str) -> Option<Connection> {
    connection(name, Credential::Typed(Secret::new(password).ok()?))
}

pub(crate) fn asked(connections: Vec<Connection>) -> RemoteMessage {
    RemoteMessage::Cmds(Cmds {
        cmds: connections.into_iter().map(RemoteCmd::Connect).collect(),
        at: Instant::now(),
    })
}

pub(crate) fn session() -> Option<Session> {
    Some(Session::new(
        Endpoint::parse("https://music.example").ok()?,
        "u=alice",
    ))
}

pub(crate) fn connected(
    name: &str,
    stored: Result<(), RemoteError>,
) -> Option<RemoteMessage> {
    Some(RemoteMessage::Connected {
        server_name: ServerName::new(name),
        result: Ok(session()?),
        stored,
    })
}

pub(crate) fn answered(
    driver: &mut RemoteDriver,
    message: RemoteMessage,
) -> Option<Answer> {
    driver.transition(message).ok().map(Cmd::into_parts)
}

pub(crate) fn jobs(
    answer: Option<Answer>,
) -> Option<(Vec<RemoteJob>, Vec<RemoteEvent>)> {
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

#[test]
fn a_forget_runs_one_job_and_its_answer_reports_a_keychain_error() {
    let Some(account) = stored("home").map(|connection| connection.account) else {
        panic!("a valid account");
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let error = RemoteError::Keychain {
        server_name: ServerName::new("home"),
        source: IoError::from(ErrorKind::Other),
    };

    let forget = answered(
        &mut driver,
        RemoteMessage::Cmds(Cmds {
            cmds: vec![RemoteCmd::Forget(account.clone())],
            at: Instant::now(),
        }),
    );
    let forgotten = answered(&mut driver, RemoteMessage::Forgotten(Err(error.clone())));

    assert_eq!(
        jobs(forget),
        Some((vec![RemoteJob::Forget(account)], Vec::new()))
    );
    assert_eq!(
        jobs(forgotten),
        Some((Vec::new(), vec![RemoteEvent::Error(error)]))
    );
    assert!(driver.transition(RemoteMessage::Forgotten(Ok(()))).is_err());
}

pub(crate) fn online(name: &str) -> Option<RemoteEvent> {
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
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"))
            .transition(asked(vec![])),
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
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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

pub(crate) fn list_cmd(
    name: &str,
    listing: Listing,
    revision: Revision,
) -> Option<RemoteMessage> {
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

pub(crate) fn online_driver(name: &str) -> RemoteDriver {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
                result: Ok((vec![], Favorites::default())),
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
                favorites: Favorites::default(),
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
            result: Ok((vec![], Favorites::default())),
            revision: first.next(),
        }),
        Err(Unhandled)
    ));
    assert_eq!(format!("{driver:?}"), before);
}

#[test]
fn a_list_with_no_connect_before_it_runs_from_the_orders_session() {
    let revision = Revision::default().next();
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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

pub(crate) fn search_cmd(input: &str, revision: Revision) -> Option<RemoteMessage> {
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

pub(crate) type Waits = (Vec<(Duration, RemoteTimer)>, Vec<RemoteEvent>);

pub(crate) fn waits(answer: Option<Answer>) -> Option<Waits> {
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

pub(crate) fn elapsed(revision: Revision) -> RemoteMessage {
    RemoteMessage::Elapsed(RemoteTimer::Search(revision))
}

#[test]
fn three_searches_within_the_wait_give_one_job_with_the_last_input() {
    let first = Revision::default().next();
    let second = first.next();
    let third = second.next();
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
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
                result: Ok((vec![], Favorites::default())),
                revision: second,
            }]
        ))
    );
    assert!(matches!(driver.transition(elapsed(first)), Err(Unhandled)));
}

#[test]
fn a_found_answer_gives_its_rows_and_a_failed_search_answers_found_with_its_revision() {
    let revision = Revision::default().next();
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    assert_eq!(
        jobs(answered(
            &mut driver,
            RemoteMessage::Found {
                server_name: ServerName::new("a"),
                result: Ok((vec![], Favorites::default())),
                revision,
            }
        )),
        Some((
            vec![],
            vec![RemoteEvent::Found {
                server_name: ServerName::new("a"),
                result: Ok((vec![], Favorites::default())),
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
        Some((
            vec![],
            vec![RemoteEvent::Found {
                server_name: ServerName::new("a"),
                result: Err(remote_error),
                revision,
            }]
        ))
    );
}

#[test]
fn a_search_with_no_connect_before_it_runs_from_the_orders_session() {
    let revision = Revision::default().next();
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    assert!(
        search_cmd("mil", revision)
            .is_some_and(|message| driver.transition(message).is_ok())
    );
    assert_eq!(
        jobs(answered(&mut driver, elapsed(revision))),
        Some((search_job("mil", revision).into_iter().collect(), vec![]))
    );
}

pub(crate) fn star_cmd(id: &str, favorite: Favorite) -> Option<RemoteCmd> {
    Some(RemoteCmd::Star {
        server_name: ServerName::new("a"),
        session: session()?,
        server_track_id: ServerTrackId::new(id),
        favorite,
    })
}

pub(crate) fn star_job(id: &str, favorite: Favorite) -> Option<RemoteJob> {
    Some(RemoteJob::Star {
        server_name: ServerName::new("a"),
        session: session()?,
        server_track_id: ServerTrackId::new(id),
        favorite,
    })
}

pub(crate) fn starred(
    id: &str,
    favorite: Favorite,
    result: Result<(), RemoteError>,
) -> RemoteMessage {
    RemoteMessage::Starred {
        server_name: ServerName::new("a"),
        server_track_id: ServerTrackId::new(id),
        favorite,
        result,
    }
}

pub(crate) fn holds(id: &str, favorite: Favorite) -> RemoteEvent {
    RemoteEvent::Starred {
        server_name: ServerName::new("a"),
        server_track_id: ServerTrackId::new(id),
        favorite,
    }
}

#[test]
fn two_stars_are_sent_in_order_and_a_failed_one_answers_the_state_before() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let remote_error = RemoteError::Moved {
        server_name: ServerName::new("a"),
    };
    let cmds = [
        star_cmd("c41d", Favorite::Yes),
        star_cmd("e7a2", Favorite::No),
    ]
    .into_iter()
    .flatten()
    .collect();

    let asked = jobs(answered(
        &mut driver,
        RemoteMessage::Cmds(Cmds {
            cmds,
            at: Instant::now(),
        }),
    ));
    let first = jobs(answered(
        &mut driver,
        starred("c41d", Favorite::Yes, Ok(())),
    ));
    let second = jobs(answered(
        &mut driver,
        starred("e7a2", Favorite::No, Err(remote_error.clone())),
    ));

    assert_eq!(
        asked,
        Some((
            star_job("c41d", Favorite::Yes).into_iter().collect(),
            vec![]
        ))
    );
    assert_eq!(
        first,
        Some((
            star_job("e7a2", Favorite::No).into_iter().collect(),
            vec![holds("c41d", Favorite::Yes)]
        ))
    );
    assert_eq!(
        second,
        Some((
            vec![],
            vec![
                holds("e7a2", Favorite::Yes),
                RemoteEvent::Error(remote_error)
            ]
        ))
    );
    assert_eq!(
        driver
            .transition(starred("e7a2", Favorite::No, Ok(())))
            .err(),
        Some(Unhandled)
    );
}

#[test]
fn a_prefetch_job_keeps_the_current_fetch_and_a_fetch_keeps_the_prefetch() {
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let key =
        |id| CacheKey::new(&ServerName::new("home"), &ServerTrackId::new(id), "flac");
    let fetch = |id| {
        Some(MediaFetch {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new(id),
            cache_key: key(id),
            session: session()?,
            first_byte: 0,
            revision: Revision::default(),
        })
    };
    let mut order = |cmd: Option<RemoteCmd>| {
        cmd.and_then(|cmd| {
            answered(
                &mut driver,
                RemoteMessage::Cmds(Cmds {
                    cmds: vec![cmd],
                    at: Instant::now(),
                }),
            )
        })
    };
    let media_dir = Arc::from(env::temp_dir().as_path());
    let kept = vec![key("a"), key("b")];
    assert!(order(fetch("a").map(RemoteCmd::Fetch)).is_some());
    assert_eq!(
        jobs(order(fetch("b").map(RemoteCmd::Prefetch))),
        fetch("b").map(|media_fetch| (
            vec![RemoteJob::Prefetch {
                media_fetch,
                media_dir: Arc::clone(&media_dir),
                kept_cache_keys: kept.clone(),
            }],
            vec![]
        ))
    );
    assert_eq!(
        jobs(order(fetch("a").map(RemoteCmd::Fetch))),
        fetch("a").map(|media_fetch| (
            vec![RemoteJob::Fetch {
                media_fetch,
                media_dir,
                kept_cache_keys: kept,
            }],
            vec![]
        ))
    );
}
