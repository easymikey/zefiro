use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        io_error::IoError,
        server::{PlayReport, Server, ServerName, ServerStatus},
        toast::Toast,
    },
    message::Message,
    update::machine::Unhandled,
};

pub(crate) fn restored(
    play_reports: &mut Vec<PlayReport>,
    result: Result<Vec<PlayReport>, IoError>,
) -> Result<Cmd, Unhandled> {
    match result {
        Ok(restored_play_reports) if restored_play_reports.is_empty() => Err(Unhandled),
        Ok(restored_play_reports) => {
            for play_report in restored_play_reports {
                if !play_reports.contains(&play_report) {
                    play_reports.push(play_report);
                }
            }
            Ok(Cmd::none())
        }
        Err(io_error) => Ok(Cmd::message(Message::Toast(Toast::error(format!(
            "Play reports are unreadable: {io_error}"
        ))))),
    }
}

pub(crate) fn forget(play_reports: &mut Vec<PlayReport>, server_name: &ServerName) {
    play_reports.retain(|play_report| play_report.server_name != *server_name);
}

pub(crate) fn ordered(servers: &[Server], play_reports: &mut Vec<PlayReport>) -> Cmd {
    let online = |play_report: &PlayReport| {
        servers
            .iter()
            .find(|server| server.account.server_name == play_report.server_name)
            .and_then(|server| match &server.server_status {
                ServerStatus::Online(session) => Some(session),
                ServerStatus::Offline(_error) => None,
                ServerStatus::Connecting => None,
            })
    };
    play_reports
        .extract_if(.., |play_report| online(play_report).is_some())
        .filter_map(|play_report| {
            let session = online(&play_report)?.clone();
            Some(Effect::Remote(RemoteCmd::Report {
                session,
                play_report,
            }))
        })
        .collect()
}
