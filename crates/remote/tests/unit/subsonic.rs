use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
};

use kernel::domain::{
    favorites::Favorite,
    revision::Revision,
    server::{
        Account,
        AlbumOrder,
        ApiCode,
        Connection,
        Credential,
        Endpoint,
        HttpStatus,
        Listing,
        Page,
        PlayReport,
        RemoteError,
        Scrobble,
        Secret,
        ServerName,
        ServerTrackId,
        Session,
        UserName,
    },
    time::Moment,
};
use remote::{
    http::{API_BYTES, agent},
    job::{RemoteJob, SignedReport},
    message::RemoteMessage,
    subsonic::ping,
};

pub(crate) const OK_BODY: &str =
    r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;

pub(crate) const FAILED_BODY: &str = r#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"Wrong username or password"}}}"#;

pub(crate) fn answer(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}

pub(crate) fn serve(answers: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        return (String::new(), thread::spawn(Vec::new));
    };
    let port = listener.local_addr().map_or(0, |addr| addr.port());
    let handle = thread::spawn(move || {
        answers
            .into_iter()
            .filter_map(|reply| {
                let (mut stream, _addr) = listener.accept().ok()?;
                let mut request_line = String::new();
                let mut reader = BufReader::new(stream.try_clone().ok()?);
                reader.read_line(&mut request_line).ok()?;
                let mut header = String::new();
                while reader.read_line(&mut header).ok()? > 2 {
                    header.clear();
                }
                stream.write_all(reply.as_bytes()).ok()?;
                Some(request_line)
            })
            .collect()
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn connection(link: &str) -> Option<Connection> {
    Some(Connection {
        account: Account {
            server_name: ServerName::new("home"),
            endpoint: Endpoint::parse(link).ok()?,
            user_name: UserName::new("alice").ok()?,
        },
        credential: Credential::Stored,
    })
}

fn pinged(link: &str) -> Option<Result<Session, RemoteError>> {
    let secret = Secret::new("hunter2").ok()?;
    Some(ping(&agent(), &connection(link)?, &secret))
}

pub(crate) fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
}

#[test]
fn ok_gives_a_session_signed_with_the_salted_secret() {
    let (link, handle) = serve(vec![answer("200 OK", "", OK_BODY)]);
    let query = pinged(&link)
        .and_then(Result::ok)
        .map(|session| session.query.to_string());
    let value = |key| query.as_deref().and_then(|query| query_value(query, key));
    let token =
        value("s").map(|salt| format!("{:x}", md5::compute(format!("hunter2{salt}"))));
    assert_eq!(value("u"), Some("alice"));
    assert_eq!(value("s").map(str::len), Some(12));
    assert_eq!(value("t"), token.as_deref());
    let requests = handle.join();
    assert!(requests.as_ref().is_ok_and(|requests| {
        requests
            .first()
            .is_some_and(|line| line.starts_with("GET /rest/ping?"))
    }));
}

#[test]
fn failed_with_code_40_is_a_credentials_error() {
    let (link, handle) = serve(vec![answer("200 OK", "", FAILED_BODY)]);
    let refused = pinged(&link).and_then(Result::err);
    assert!(
        refused.as_ref().is_some_and(RemoteError::is_credentials),
        "{refused:?}"
    );
    assert!(handle.join().is_ok());
}

#[test]
fn service_unavailable_is_a_status_error() {
    let (link, handle) = serve(vec![answer("503 Service Unavailable", "", "")]);
    let refused = pinged(&link).and_then(Result::err);
    assert_eq!(
        refused,
        Some(RemoteError::Status {
            server_name: ServerName::new("home"),
            http_status: HttpStatus(503),
        })
    );
    assert!(handle.join().is_ok());
}

#[test]
fn a_closed_port_is_unreachable() {
    let (link, handle) = serve(Vec::new());
    assert!(handle.join().is_ok());
    let refused = pinged(&link).and_then(Result::err);
    assert!(
        matches!(refused, Some(RemoteError::Unreachable { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_redirect_to_another_host_is_moved() {
    let (link, handle) = serve(vec![answer(
        "302 Found",
        "Location: http://elsewhere.example/rest/ping\r\n",
        "",
    )]);
    let refused = pinged(&link).and_then(Result::err);
    assert_eq!(
        refused,
        Some(RemoteError::Moved {
            server_name: ServerName::new("home"),
        })
    );
    assert!(handle.join().is_ok());
}

#[test]
fn a_redirect_to_the_same_host_and_path_is_followed() {
    let (link, handle) = serve(vec![
        answer("301 Moved Permanently", "Location: /rest/ping\r\n", ""),
        answer("200 OK", "", OK_BODY),
    ]);
    let session = pinged(&link).and_then(Result::ok);
    assert!(session.is_some());
    let requests = handle.join();
    assert!(requests.as_ref().is_ok_and(|requests| requests.len() == 2
        && requests.iter().all(|line| line.contains("&s="))));
}

#[test]
fn a_redirect_with_its_own_query_is_followed_and_signed() {
    let (link, handle) = serve(vec![
        answer("302 Found", "Location: /rest/ping?x=1\r\n", ""),
        answer("200 OK", "", OK_BODY),
    ]);
    let session = pinged(&link).and_then(Result::ok);
    assert!(session.is_some());
    let requests = handle.join();
    assert!(
        requests
            .as_ref()
            .is_ok_and(|requests| requests
                .get(1)
                .is_some_and(|line| line.starts_with("GET /rest/ping?x=1&")
                    && line.contains("&s="))),
        "{requests:?}"
    );
}

#[test]
fn a_protocol_relative_redirect_is_moved() {
    let (link, handle) = serve(vec![answer(
        "302 Found",
        &format!("Location: {}elsewhere.example/rest/ping\r\n", "/".repeat(2)),
        "",
    )]);
    let refused = pinged(&link).and_then(Result::err);
    assert_eq!(
        refused,
        Some(RemoteError::Moved {
            server_name: ServerName::new("home"),
        })
    );
    assert!(handle.join().is_ok());
}

#[test]
fn a_fourth_redirect_is_moved() {
    let (link, handle) =
        serve(vec![answer("302 Found", "Location: /rest/ping\r\n", ""); 4]);
    let refused = pinged(&link).and_then(Result::err);
    assert_eq!(
        refused,
        Some(RemoteError::Moved {
            server_name: ServerName::new("home"),
        })
    );
    assert!(handle.join().is_ok_and(|requests| requests.len() == 4));
}

#[test]
fn a_body_over_the_limit_is_a_parse_error() {
    let body = " ".repeat(usize::try_from(API_BYTES).unwrap_or(usize::MAX) + 1);
    let (link, handle) = serve(vec![answer("200 OK", "", &body)]);
    let refused = pinged(&link).and_then(Result::err);
    assert!(
        matches!(refused, Some(RemoteError::Parse { .. })),
        "{refused:?}"
    );
    assert!(handle.join().is_ok());
}

#[test]
fn a_redirected_list_keeps_the_auth_query_and_drops_its_own_parameters() {
    let (link, handle) = serve(vec![
        answer("302 Found", "Location: /rest/moved?x=1\r\n", ""),
        answer("200 OK", "", OK_BODY),
    ]);
    let message = Endpoint::parse(&link).ok().map(|endpoint| {
        RemoteJob::List {
            server_name: ServerName::new("home"),
            session: Session::new(endpoint, "u=alice&t=token&s=salt"),
            listing: Listing::Albums(AlbumOrder::Newest),
            page: Page::default(),
            revision: Revision::default(),
        }
        .run(&agent())
    });
    assert!(message.is_some());
    let requests = handle.join().unwrap_or_else(|_panic| Vec::new());
    let first = requests.first().map_or("", String::as_str);
    let second = requests.get(1).map_or("", String::as_str);
    assert!(first.contains("type=newest"), "{first}");
    assert!(
        second.starts_with("GET /rest/moved?x=1&u=alice&t=token&s=salt "),
        "{second}"
    );
    assert!(!second.contains("type="), "{second}");
}

fn star(link: &str, favorite: Favorite) -> Option<RemoteJob> {
    Some(RemoteJob::Star {
        server_name: ServerName::new("home"),
        session: Session::new(Endpoint::parse(link).ok()?, "u=bob&t=token&s=salt"),
        server_track_id: ServerTrackId::new("c41d"),
        favorite,
    })
}

#[test]
fn a_star_asks_star_and_an_unstar_asks_unstar_and_a_failed_one_is_an_error() {
    let (link, handle) = serve(vec![
        answer("200 OK", "", OK_BODY),
        answer("200 OK", "", FAILED_BODY),
    ]);
    let results: Vec<Result<(), RemoteError>> = [Favorite::Yes, Favorite::No]
        .into_iter()
        .filter_map(|favorite| star(&link, favorite))
        .map(|job| {
            let RemoteMessage::Starred { result, .. } = job.run(&agent()) else {
                panic!("a star job answers Starred");
            };
            result
        })
        .collect();
    assert_eq!(
        results,
        [
            Ok(()),
            Err(RemoteError::Api {
                server_name: ServerName::new("home"),
                api_code: ApiCode(40),
            }),
        ]
    );
    let requests = handle.join().unwrap_or_else(|_panic| Vec::new());
    let queries: Vec<(&str, Option<&str>)> = requests
        .iter()
        .filter_map(|request| request.split_once('?'))
        .map(|(path, query)| (path, query_value(query, "id")))
        .collect();
    assert_eq!(
        queries,
        [
            ("GET /rest/star", Some("c41d")),
            ("GET /rest/unstar", Some("c41d"))
        ]
    );
}

fn report(link: &str, scrobble: Scrobble) -> Option<RemoteJob> {
    Some(RemoteJob::Report(vec![SignedReport {
        session: Session::new(Endpoint::parse(link).ok()?, "u=bob&t=token&s=salt"),
        play_report: PlayReport {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("c41d"),
            scrobble,
        },
    }]))
}

#[test]
fn now_playing_asks_no_submission_and_played_submits_its_time() {
    let (link, handle) = serve(vec![
        answer("200 OK", "", OK_BODY),
        answer("200 OK", "", FAILED_BODY),
    ]);
    let moment = Moment::new(std::time::Duration::from_millis(1_700_000_000_123));
    let remote_messages: Vec<RemoteMessage> =
        [Scrobble::NowPlaying, Scrobble::Played(moment)]
            .into_iter()
            .filter_map(|scrobble| report(&link, scrobble))
            .map(|job| job.run(&agent()))
            .collect();
    let requests = handle.join().unwrap_or_else(|_panic| Vec::new());
    let queries: Vec<(&str, [Option<&str>; 3])> = requests
        .iter()
        .filter_map(|request| request.split_once('?'))
        .map(|(path, query)| {
            (
                path,
                ["id", "time", "submission"].map(|key| query_value(query, key)),
            )
        })
        .collect();

    assert!(matches!(
        remote_messages.as_slice(),
        [
            RemoteMessage::Reported {
                play_reports: accepted,
                result: Ok(()),
            },
            RemoteMessage::Reported {
                play_reports: refused,
                result: Err(RemoteError::Api { .. }),
            }
        ] if accepted.len() == 1 && refused.is_empty()
    ));
    assert_eq!(
        queries,
        [
            ("GET /rest/scrobble", [Some("c41d"), None, Some("false")]),
            (
                "GET /rest/scrobble",
                [Some("c41d"), Some("1700000000123"), Some("true")]
            ),
        ]
    );
}
