use kernel::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        catalog::CatalogName,
        io_error::IoError,
        model::Model,
        server::{PlayReport, Scrobble, ServerStatus, ServerTrackId},
        time::Moment,
    },
    message::{Message, RemoteEvent, ServerRequest},
    update::machine::Unhandled,
};

use crate::{
    support::update::update,
    table::server_tab::{home, online, server_model, session},
};

fn play_report() -> PlayReport {
    PlayReport {
        server_name: home(),
        server_track_id: ServerTrackId::new("c41d"),
        scrobble: Scrobble::Played(Moment::default()),
    }
}

fn local_model(server_status: ServerStatus) -> Model {
    let mut model = server_model(server_status, 0);
    model.catalog_name = CatalogName::Local;
    model
}

fn restored(
    model: &mut Model,
    result: Result<Vec<PlayReport>, IoError>,
) -> Result<Cmd, Unhandled> {
    update(
        model,
        Message::Remote(RemoteEvent::Restored(result)),
        Moment::default(),
    )
}

fn ordered() -> Result<Cmd, Unhandled> {
    Ok(Cmd::from(Effect::Remote(RemoteCmd::Report {
        session: session(),
        play_report: play_report(),
    })))
}

#[test]
fn restored_reports_wait_in_the_model_while_their_server_is_not_online() {
    let mut model = local_model(ServerStatus::Connecting);

    let answer = restored(&mut model, Ok(vec![play_report()]));

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(model.play_reports, vec![play_report()]);
}

#[test]
fn restored_reports_are_ordered_again_with_the_session_once_their_server_is_online() {
    let mut model = local_model(ServerStatus::Connecting);
    assert_eq!(
        restored(&mut model, Ok(vec![play_report()])),
        Ok(Cmd::none())
    );

    let answer = update(
        &mut model,
        Message::Remote(RemoteEvent::Connected {
            server_name: home(),
            session: session(),
        }),
        Moment::default(),
    );

    assert_eq!(answer, ordered());
    assert!(model.play_reports.is_empty());
}

#[test]
fn restored_reports_of_an_online_server_are_ordered_at_once() {
    let mut model = local_model(online());

    assert_eq!(restored(&mut model, Ok(vec![play_report()])), ordered());
    assert!(model.play_reports.is_empty());
}

#[test]
fn a_restored_report_the_model_already_holds_is_kept_once() {
    let mut model = local_model(ServerStatus::Connecting);
    model.play_reports = vec![play_report()];

    let answer = restored(&mut model, Ok(vec![play_report()]));

    assert_eq!(answer, Ok(Cmd::none()));
    assert_eq!(model.play_reports, vec![play_report()]);
}

#[test]
fn no_restored_reports_are_refused() {
    let mut model = local_model(ServerStatus::Connecting);

    assert_eq!(restored(&mut model, Ok(Vec::new())), Err(Unhandled));
}

#[test]
fn an_unreadable_reports_file_shows_an_error_toast() {
    let mut model = local_model(ServerStatus::Connecting);

    let answer = restored(&mut model, Err(IoError::Malformed));

    assert!(answer.is_ok_and(|cmd| cmd != Cmd::none()));
    assert!(model.play_reports.is_empty());
}

#[test]
fn an_unsaved_reports_file_shows_an_error_toast_and_leaves_the_server_online() {
    let mut model = local_model(online());

    let answer = update(
        &mut model,
        Message::Remote(RemoteEvent::Unsaved(IoError::Denied)),
        Moment::default(),
    );

    assert!(answer.is_ok_and(|cmd| cmd != Cmd::none()));
    assert_eq!(model.servers[0].server_status, online());
}

#[test]
fn quit_flushes_the_reports_the_model_still_holds() {
    let mut model = local_model(ServerStatus::Connecting);
    model.play_reports = vec![play_report()];

    let (effects, _messages) = update(&mut model, Message::Quit, Moment::default())
        .unwrap()
        .into_parts();

    assert!(effects.contains(&Effect::Remote(RemoteCmd::Flush(vec![play_report()]))));
}

#[test]
fn remove_of_a_server_drops_its_restored_reports_from_the_flush_at_quit() {
    let mut model = local_model(ServerStatus::Connecting);
    assert_eq!(
        restored(&mut model, Ok(vec![play_report()])),
        Ok(Cmd::none())
    );
    assert!(
        update(
            &mut model,
            Message::Server(ServerRequest::Remove(home())),
            Moment::default(),
        )
        .is_ok()
    );

    let (effects, _messages) = update(&mut model, Message::Quit, Moment::default())
        .unwrap()
        .into_parts();

    assert!(effects.contains(&Effect::Remote(RemoteCmd::Flush(Vec::new()))));
}
