use crate::{
    cmd::{Cmd, Cue},
    domain::{Model, Reply, Revision, Workspace},
    message::Timer,
    update::{machine::Machine, player::PlayerMessage, rejection::Rejection},
};

pub(super) fn update(model: &mut Model, timer: Timer) -> Result<Cmd, Rejection> {
    match timer {
        Timer::Toast(revision) => Ok(toast_expired(
            &mut model.workspace,
            revision.reply(model.toast_generation),
        )),
        Timer::Sleep(revision) => sleep_fired(model, revision),
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

fn sleep_fired(model: &mut Model, revision: Revision) -> Result<Cmd, Rejection> {
    match (
        revision.reply(model.sleep_generation),
        model.transport.sleep,
    ) {
        (Reply::Awaited, Some(_)) => {
            let paused = model.player.update(PlayerMessage::SleepFired)?;
            model.transport.sleep = None;
            Ok(paused)
        }
        (Reply::Awaited, None) | (Reply::Stale, Some(_) | None) => Ok(Cmd::None),
    }
}
