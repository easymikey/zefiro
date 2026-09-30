use crate::{
    cmd::{Cmd, Cue},
    domain::{Model, Moment, Reply, Revision, Workspace},
    message::Timer,
    update::{audio, driver, error::UpdateError, player, player::PlayerMessage},
};

pub(crate) fn update(
    model: &mut Model,
    timer: Timer,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match timer {
        Timer::Toast(revision) => Ok(toast_expired(
            &mut model.workspace,
            revision.reply(model.revisions.toast),
        )),
        Timer::Sleep(revision) => sleep_fired(model, revision, now),
        Timer::Mark(revision) => audio::mark_fired(model, revision, now),
        Timer::Restart(restarting) => Ok(driver::restart_due(model, restarting, now)),
    }
}

fn toast_expired(workspace: &mut Workspace, reply: Reply) -> Cmd {
    match reply {
        Reply::Awaited => workspace
            .toast
            .take()
            .map_or(Cmd::None, |_| Cue::ToastDismissed.into()),
        Reply::Stale => Cmd::None,
    }
}

fn sleep_fired(
    model: &mut Model,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match (revision.reply(model.revisions.sleep), model.transport.sleep) {
        (Reply::Awaited, Some(_)) => {
            let paused =
                player::update_player(model, PlayerMessage::SleepFired(now), now)?;
            model.transport.sleep = None;
            Ok(paused)
        }
        (Reply::Awaited, None) | (Reply::Stale, Some(_) | None) => Ok(Cmd::None),
    }
}
