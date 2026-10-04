use crate::{
    cmd::Cmd,
    domain::{
        revision::{Freshness, Revision},
        time::Moment,
        workspace::Workspace,
    },
    update::{
        machine::Unhandled,
        player,
        player::{PlaybackParts, PlayerMessage},
    },
};

pub(crate) fn toast_expired(
    workspace: &mut Workspace,
    revision: Revision,
    reply: Freshness,
) -> Result<Cmd, Unhandled> {
    match reply {
        Freshness::Awaited => Ok(workspace.expire(revision)),
        Freshness::Stale => Err(Unhandled),
    }
}

pub(crate) fn sleep_fired(
    playback: &mut PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match (
        revision.freshness(playback.revisions.sleep),
        playback.transport.sleep,
    ) {
        (Freshness::Awaited, Some(_)) => {
            let paused =
                player::update_player(playback, PlayerMessage::SleepFired(now), now)?;
            playback.transport.sleep = None;
            Ok(paused)
        }
        (Freshness::Awaited, None) | (Freshness::Stale, Some(_) | None) => {
            Err(Unhandled)
        }
    }
}
