use std::{env, time::Instant};

use kernel::{
    cmd::{Cmds, RemoteCmd},
    domain::{
        favorites::{Favorite, Favorites},
        revision::Revision,
        server::{Account, AlbumOrder, Listing, Page, ServerName},
    },
    message::RemoteEvent,
    update::machine::{Machine, Unhandled},
};
use remote::{driver::RemoteDriver, job::RemoteJob, message::RemoteMessage};

use crate::unit::driver::{
    answered,
    asked,
    connected,
    elapsed,
    holds,
    jobs,
    list_cmd,
    online,
    online_driver,
    search_cmd,
    star_cmd,
    star_job,
    starred,
    stored,
    typed,
};

fn forgets(account: Account) -> RemoteMessage {
    RemoteMessage::Cmds(Cmds {
        cmds: vec![RemoteCmd::Forget(account)],
        at: Instant::now(),
    })
}

#[test]
fn a_forget_drops_the_server_waiting_connect() {
    let (Some(a), Some(x)) = (stored("a"), typed("x", "secret")) else {
        panic!("valid connections");
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let connects = jobs(answered(&mut driver, asked(vec![a.clone(), x.clone()])));
    let forget = jobs(answered(&mut driver, forgets(x.account.clone())));
    let online_a =
        jobs(connected("a", Ok(())).and_then(|message| answered(&mut driver, message)));

    assert_eq!(connects, Some((vec![RemoteJob::Connect(a)], vec![])));
    assert_eq!(forget, Some((vec![RemoteJob::Forget(x.account)], vec![])));
    assert_eq!(online_a, Some((vec![], online("a").into_iter().collect())));
}

#[test]
fn a_forget_of_the_server_connecting_runs_after_its_connect_answers() {
    let Some(x) = typed("x", "secret") else {
        panic!("a valid connection");
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let connects = jobs(answered(&mut driver, asked(vec![x.clone()])));
    let forget = jobs(answered(&mut driver, forgets(x.account.clone())));
    let online_x =
        jobs(connected("x", Ok(())).and_then(|message| answered(&mut driver, message)));
    let forgotten = jobs(answered(&mut driver, RemoteMessage::Forgotten(Ok(()))));

    assert_eq!(
        connects,
        Some((vec![RemoteJob::Connect(x.clone())], vec![]))
    );
    assert_eq!(forget, Some((vec![], vec![])));
    assert_eq!(
        online_x,
        Some((
            vec![RemoteJob::Forget(x.account)],
            online("x").into_iter().collect()
        ))
    );
    assert_eq!(forgotten, Some((vec![], vec![])));
}

#[test]
fn a_connect_waits_for_the_forget_of_its_server() {
    let Some(x) = stored("x") else {
        panic!("a valid connection");
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));

    let forget = jobs(answered(&mut driver, forgets(x.account.clone())));
    let connects = jobs(answered(&mut driver, asked(vec![x.clone()])));
    let forgotten = jobs(answered(&mut driver, RemoteMessage::Forgotten(Ok(()))));

    assert_eq!(
        forget,
        Some((vec![RemoteJob::Forget(x.account.clone())], vec![]))
    );
    assert_eq!(connects, Some((vec![], vec![])));
    assert_eq!(forgotten, Some((vec![RemoteJob::Connect(x)], vec![])));
}

#[test]
fn a_forget_drops_the_server_queued_star_and_runs_after_the_star_in_flight() {
    let Some(account) = stored("a").map(|connection| connection.account) else {
        panic!("a valid account");
    };
    let mut driver =
        RemoteDriver::new(env::temp_dir(), env::temp_dir().join("sifr-reports.json"));
    let cmds = [
        star_cmd("c41d", Favorite::Yes),
        star_cmd("e7a2", Favorite::No),
        Some(RemoteCmd::Forget(account.clone())),
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
            vec![RemoteJob::Forget(account)],
            vec![holds("c41d", Favorite::Yes)]
        ))
    );
}

#[test]
fn a_forget_drops_the_server_waiting_list_and_search() {
    let first = Revision::default().next();
    let second = first.next();
    let third = second.next();
    let mut driver = online_driver("a");
    let Some(account) = stored("a").map(|connection| connection.account) else {
        panic!("a valid account");
    };

    let listing = [
        list_cmd("a", Listing::Albums(AlbumOrder::Newest), first),
        list_cmd("a", Listing::Albums(AlbumOrder::Random), second),
        search_cmd("mil", third),
    ]
    .into_iter()
    .flatten()
    .all(|message| driver.transition(message).is_ok());
    let forget = jobs(answered(&mut driver, forgets(account.clone())));
    let listed = jobs(answered(
        &mut driver,
        RemoteMessage::Listed {
            server_name: ServerName::new("a"),
            listing: Listing::Albums(AlbumOrder::Newest),
            page: Page::default(),
            result: Ok((vec![], Favorites::default())),
            revision: first,
        },
    ));

    assert!(listing);
    assert_eq!(forget, Some((vec![RemoteJob::Forget(account)], vec![])));
    assert_eq!(
        listed,
        Some((
            vec![],
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
    assert_eq!(driver.transition(elapsed(third)).err(), Some(Unhandled));
}
